use super::*;

pub(super) fn exercise_pre_export_cleanup(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
) -> Result<(), String> {
    let skipped_outcomes = BTreeMap::from([
        ("restore_outcome".to_owned(), "skipped".to_owned()),
        ("import_outcome".to_owned(), "skipped".to_owned()),
        ("export_outcome".to_owned(), "skipped".to_owned()),
    ]);
    let abandoned = fixture.stage("abandoned-before-export", "810014", false)?;
    let expected = abandoned
        .preflight_outputs
        .get("bundle_path_identity")
        .ok_or("preflight omitted bundle identity")?;
    if &path_identity(&abandoned.bundle)? != expected
        || lifecycle.save[2].condition.as_deref()
            != Some("always() && steps.mbx_cache_preflight.outcome == 'success'")
    {
        return Err("pre-export cleanup lacks its reserved bundle authority".to_owned());
    }
    cleanup_after_export(lifecycle, &abandoned, &skipped_outcomes)?;

    reject_symlink_replacement(fixture, lifecycle, &skipped_outcomes)?;
    reject_inode_replacement(fixture, lifecycle, &skipped_outcomes)
}

fn reject_symlink_replacement(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
    skipped_outcomes: &BTreeMap<String, String>,
) -> Result<(), String> {
    let replaced = fixture.stage("replaced-before-cleanup", "810015", false)?;
    let external = replaced.root.join("replacement-target");
    fs::create_dir(&external).map_err(|err| err.to_string())?;
    let sentinel = external.join("sentinel");
    fs::write(&sentinel, b"replacement owner").map_err(|err| err.to_string())?;
    fs::remove_dir(&replaced.bundle).map_err(|err| err.to_string())?;
    symlink(&external, &replaced.bundle).map_err(|err| err.to_string())?;
    let rejected = run_shell(
        &lifecycle.save[2],
        &replaced,
        skipped_outcomes,
        &replaced.project,
        &replaced.temp,
        &[],
        "replaced-bundle-cleanup",
    )?;
    if rejected.success
        || !rejected
            .text
            .contains("preflight bundle identity changed before restore")
        || !fs::symlink_metadata(&replaced.bundle)
            .map_err(|err| err.to_string())?
            .file_type()
            .is_symlink()
        || fs::read(&sentinel).map_err(|err| err.to_string())? != b"replacement owner"
        || !replaced.project.join(".velnor-mbx-cache").is_dir()
        || !replaced.project.join(".velnor-mbx-cache.lock").is_dir()
    {
        return Err(format!(
            "cleanup followed or removed a replacement bundle path: {}",
            rejected.text
        ));
    }
    Ok(())
}

fn reject_inode_replacement(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
    skipped_outcomes: &BTreeMap<String, String>,
) -> Result<(), String> {
    let stage = fixture.stage("replaced-inode-before-cleanup", "810017", false)?;
    let original_identity = stage
        .preflight_outputs
        .get("bundle_path_identity")
        .ok_or("preflight omitted bundle identity")?
        .clone();
    let retired = stage.root.join("retired-bundle-reservation");
    fs::rename(&stage.bundle, &retired).map_err(|err| err.to_string())?;
    fs::create_dir(&stage.bundle).map_err(|err| err.to_string())?;
    fs::set_permissions(&stage.bundle, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    let replacement_identity = path_identity(&stage.bundle)?;
    let rejected = run_shell(
        &lifecycle.save[2],
        &stage,
        skipped_outcomes,
        &stage.project,
        &stage.temp,
        &[],
        "replaced-inode-bundle-cleanup",
    )?;
    if replacement_identity == original_identity
        || rejected.success
        || !rejected
            .text
            .contains("preflight bundle identity changed before restore")
        || path_identity(&stage.bundle)? != replacement_identity
        || !retired.is_dir()
        || !stage.project.join(".velnor-mbx-cache.lock").is_dir()
    {
        return Err(format!(
            "cleanup accepted or removed a new canonical bundle inode: {}",
            rejected.text
        ));
    }
    Ok(())
}
