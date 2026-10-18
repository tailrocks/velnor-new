#[test]
fn racing_foreign_file_wins_without_being_unlinked() -> Result<(), String> {
    let scratch = test_scratch("velnor-service-race-test")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let staged = stage
        .create_payload(b"Velnor's plist")
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");
    let expected = b"Velnor's plist";
    let foreign = b"foreign process won the publication race";
    std::fs::write(&destination, foreign).map_err(|error| error.to_string())?;

    let result = publish_staged(&stage, &staged, &destination, expected);
    let after = std::fs::read(&destination).map_err(|error| error.to_string())?;
    stage
        .remove_payload_if_owned(&staged, expected)
        .map_err(|error| error.to_string())?;
    drop(staged);
    stage.cleanup().map_err(|error| error.to_string())?;
    if result.is_ok() || after != foreign {
        return Err("a racing file was replaced or removed".to_owned());
    }
    Ok(())
}

#[test]
fn review_destination_arrival_is_not_overwritten() -> Result<(), String> {
    let scratch = test_scratch("velnor-review-destination")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let staged = stage
        .create_payload(b"expected service")
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");
    std::fs::write(&destination, b"other service").map_err(|error| error.to_string())?;

    let result = super::publish_staged(&stage, &staged, &destination, b"expected service");
    let after = std::fs::read(&destination).map_err(|error| error.to_string())?;
    if result.is_ok() || after != b"other service" {
        return Err("publication replaced a destination created by another writer".to_owned());
    }
    stage
        .remove_payload_if_owned(&staged, b"expected service")
        .map_err(|error| error.to_string())?;
    drop(staged);
    stage.cleanup().map_err(|error| error.to_string())
}

#[test]
fn review_replaced_payload_is_preserved_and_not_published() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-review-payload")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let staged = stage
        .create_payload(b"expected service")
        .map_err(|error| error.to_string())?;
    let source = stage.path().join("payload");
    std::fs::rename(&source, stage.path().join("retained-original"))
        .map_err(|error| error.to_string())?;
    std::fs::write(&source, b"other service").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");

    let result = super::publish_staged(&stage, &staged, &destination, b"expected service");
    let actual = std::fs::read(&source).map_err(|error| error.to_string())?;
    if result.is_ok() || actual != b"other service" || destination.exists() {
        return Err("publication accepted or removed the replacement payload".to_owned());
    }
    Ok(())
}

#[test]
fn payload_cleanup_quarantines_and_preserves_a_same_content_replacement() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-payload-cleanup-race")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let expected = b"expected service";
    let owned = stage
        .create_payload(expected)
        .map_err(|error| error.to_string())?;
    let payload_path = stage.path().join("payload");
    let retained_path = stage.path().join("retained-owned-payload");
    let mut replacement_before = None;

    stage
        .remove_payload_if_owned_with(&owned, expected, || {
            std::fs::rename(&payload_path, &retained_path).expect("retain owned inode");
            std::fs::write(&payload_path, expected).expect("write same-content replacement");
            std::fs::set_permissions(&payload_path, std::fs::Permissions::from_mode(0o600))
                .expect("set private replacement mode");
            replacement_before =
                Some(std::fs::metadata(&payload_path).expect("inspect same-content replacement"));
        })
        .map_err(|error| error.to_string())?;

    let replacement = std::fs::metadata(&payload_path).map_err(|error| error.to_string())?;
    let retained = std::fs::metadata(&retained_path).map_err(|error| error.to_string())?;
    let replacement_before =
        replacement_before.ok_or_else(|| "replacement not recorded".to_owned())?;
    if std::fs::read(&payload_path).map_err(|error| error.to_string())? != expected
        || !same_file(&replacement_before, &replacement)
        || !same_file(
            &retained,
            &owned.metadata().map_err(|error| error.to_string())?,
        )
        || stage
            .payload_matches(&owned, expected)
            .map_err(|error| error.to_string())?
    {
        return Err("payload cleanup removed or accepted a same-content replacement".to_owned());
    }
    Ok(())
}

#[test]
fn review_replaced_staging_directory_is_preserved_and_rejected() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-review-directory")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let original_path = stage.path().to_path_buf();
    let staged = stage
        .create_payload(b"expected service")
        .map_err(|error| error.to_string())?;
    std::fs::rename(&original_path, scratch.path().join("retained-stage"))
        .map_err(|error| error.to_string())?;
    std::fs::create_dir(&original_path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&original_path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let replacement = original_path.join("payload");
    std::fs::write(&replacement, b"other service").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");

    let result = super::publish_staged(&stage, &staged, &destination, b"expected service");
    let actual = std::fs::read(&replacement).map_err(|error| error.to_string())?;
    if result.is_ok() || actual != b"other service" || destination.exists() {
        return Err("publication accepted or removed the replacement directory payload".to_owned());
    }
    stage
        .remove_payload_if_owned(&staged, b"expected service")
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn review_changed_staged_mode_is_rejected() -> Result<(), String> {
    let scratch = test_scratch("velnor-review-mode")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let staged = stage
        .create_payload(b"expected service")
        .map_err(|error| error.to_string())?;
    rustix::fs::fchmod(&staged, rustix::fs::Mode::from_bits_truncate(0o644))
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");

    let result = super::publish_staged(&stage, &staged, &destination, b"expected service");
    let mode = std::fs::metadata(stage.path().join("payload"))
        .map_err(|error| error.to_string())?
        .mode()
        & 0o7777;
    if result.is_ok() || mode != 0o644 || destination.exists() {
        return Err("publication accepted or changed a staged file with unsafe mode".to_owned());
    }
    Ok(())
}

#[test]
fn review_publication_remains_idempotent_before_cleanup() -> Result<(), String> {
    let scratch = test_scratch("velnor-review-idempotent")?;
    let stage =
        PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
    let staged = stage
        .create_payload(b"expected service")
        .map_err(|error| error.to_string())?;
    let destination = scratch.path().join("agent.plist");
    super::publish_staged(&stage, &staged, &destination, b"expected service")
        .map_err(|error| error.to_string())?;
    let before = std::fs::metadata(&destination).map_err(|error| error.to_string())?;
    super::atomic_write(&destination, b"expected service").map_err(|error| error.to_string())?;
    let after = std::fs::metadata(&destination).map_err(|error| error.to_string())?;
    if !super::same_file(&before, &after)
        || std::fs::read(&destination).map_err(|error| error.to_string())? != b"expected service"
    {
        return Err("repeated publication changed the installed file".to_owned());
    }
    stage
        .remove_payload_if_owned(&staged, b"expected service")
        .map_err(|error| error.to_string())?;
    drop(staged);
    stage.cleanup().map_err(|error| error.to_string())
}

#[test]
fn review_symlink_and_large_wrong_file_are_rejected() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-review-types")?;
    let target = scratch.path().join("target");
    std::fs::write(&target, b"expected service").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let link = scratch.path().join("link");
    std::os::unix::fs::symlink(&target, &link).map_err(|error| error.to_string())?;
    if super::existing_matches(&link, b"expected service").is_ok() {
        return Err("existing symlink was accepted".to_owned());
    }
    let large = scratch.path().join("large");
    std::fs::write(&large, b"expected service").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&large, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&large)
        .map_err(|error| error.to_string())?
        .set_len(256 * 1024 * 1024)
        .map_err(|error| error.to_string())?;
    if super::existing_matches(&large, b"expected service").map_err(|error| error.to_string())?
        || std::fs::metadata(&large)
            .map_err(|error| error.to_string())?
            .len()
            != 256 * 1024 * 1024
    {
        return Err("large wrong file was accepted or changed".to_owned());
    }
    Ok(())
}
