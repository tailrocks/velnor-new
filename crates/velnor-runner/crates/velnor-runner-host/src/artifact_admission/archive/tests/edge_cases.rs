use super::*;

#[test]
fn rejects_ambiguous_image_descriptors() -> Result<(), HostError> {
    let (archive, mut manifest) = fixture(None, false)?;
    let mut files = super::super::tar_archive::read_archive(&archive)?;
    let mut index: Value =
        serde_json::from_slice(files.get("index.json").ok_or(HostError::Identity)?)
            .map_err(|_| HostError::Identity)?;
    let descriptors = index
        .get_mut("manifests")
        .and_then(Value::as_array_mut)
        .ok_or(HostError::Identity)?;
    let descriptor = descriptors.first().ok_or(HostError::Identity)?.clone();
    descriptors.push(descriptor);
    files.insert(
        "index.json".to_owned(),
        serde_json::to_vec(&index).map_err(|_| HostError::Identity)?,
    );
    let ambiguous = refresh_archive(&files, &mut manifest, None)?;
    assert_eq!(validate(&ambiguous, &manifest), Err(HostError::Identity));
    Ok(())
}

#[test]
fn requires_saved_and_loaded_working_directory_to_match() -> Result<(), HostError> {
    let (wrong_saved_dir, manifest) = fixture_with_working_dir(None, false, Some("/tmp"))?;
    assert_eq!(
        validate(&wrong_saved_dir, &manifest),
        Err(HostError::Identity)
    );
    let (missing_saved_dir, manifest) = fixture_with_working_dir(None, false, None)?;
    assert_eq!(
        validate(&missing_saved_dir, &manifest),
        Err(HostError::Identity)
    );

    let (archive, manifest) = fixture(None, false)?;
    let identity = validate(&archive, &manifest)?;
    assert_eq!(
        validate_loaded_image(&identity, &manifest, &identity.config_digest, None, "/tmp",),
        Err(HostError::Identity)
    );
    Ok(())
}

#[test]
fn accepts_classic_and_containerd_inspect_identity_and_rejects_drift() -> Result<(), HostError> {
    let (archive, manifest) = fixture(None, false)?;
    let identity = validate(&archive, &manifest)?;
    validate_loaded_image(
        &identity,
        &manifest,
        &identity.config_digest,
        None,
        IMAGE_WORKDIR,
    )?;
    let descriptor = json!({
        "mediaType": identity.image_manifest_media_type.clone(),
        "digest": identity.image_manifest_digest.clone(),
        "size": i64::try_from(identity.image_manifest_size).map_err(|_| HostError::Frame)?,
    });
    validate_loaded_image(
        &identity,
        &manifest,
        &identity.image_manifest_digest,
        Some(descriptor.clone()),
        IMAGE_WORKDIR,
    )?;
    assert_eq!(
        validate_loaded_image(
            &identity,
            &manifest,
            &identity.config_digest,
            Some(descriptor.clone()),
            IMAGE_WORKDIR,
        ),
        Err(HostError::Identity)
    );
    let mut wrong_descriptor = descriptor;
    wrong_descriptor["digest"] = json!(identity.config_digest.clone());
    assert_eq!(
        validate_loaded_image(
            &identity,
            &manifest,
            &identity.image_manifest_digest,
            Some(wrong_descriptor),
            IMAGE_WORKDIR,
        ),
        Err(HostError::Identity)
    );
    assert_eq!(
        validate_loaded_image(
            &identity,
            &manifest,
            &identity.image_manifest_digest,
            Some(Value::Null),
            IMAGE_WORKDIR,
        ),
        Err(HostError::Identity)
    );
    Ok(())
}
