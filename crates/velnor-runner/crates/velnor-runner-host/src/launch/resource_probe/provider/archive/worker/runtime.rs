use bollard::models::ContainerConfig;
use serde_json::Value;

use super::super::types::{Descriptor, ImageConfigFile, LegacyManifest, OciManifest};
use super::ProviderFailure;
use super::layout::{blob_path, get_blob, valid_sha256, verify_blob, verify_descriptor};
use super::model::{
    ArchiveFiles, BASE_LABELS, OCI_CONFIG_MEDIA_TYPE, OCI_GZIP_LAYER_MEDIA_TYPE,
    OCI_MANIFEST_MEDIA_TYPE, PATH_ENV, WorkerImage,
};

pub(super) fn verify_platform_image(
    archive: &ArchiveFiles,
    image: WorkerImage,
    manifest: &OciManifest,
) -> Result<(ContainerConfig, Vec<String>, String), ProviderFailure> {
    if manifest.schema_version != 2
        || manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
        || manifest.artifact_type.is_some()
        || manifest.subject.is_some()
        || manifest.layers.is_empty()
        || manifest.layers.len() > 64
    {
        return Err(ProviderFailure::Archive);
    }
    let config_descriptor = &manifest.config;
    verify_descriptor(config_descriptor)?;
    if config_descriptor.media_type != OCI_CONFIG_MEDIA_TYPE
        || config_descriptor.artifact_type.is_some()
        || config_descriptor.annotations.is_some()
        || config_descriptor.platform.is_some()
    {
        return Err(ProviderFailure::Archive);
    }
    let config_bytes = get_blob(archive, config_descriptor)?;
    let image_config: ImageConfigFile =
        serde_json::from_slice(config_bytes).map_err(|_| ProviderFailure::Archive)?;
    if image_config.architecture != "amd64"
        || image_config.os != "linux"
        || image_config.variant.is_some()
        || image_config.rootfs.kind != "layers"
        || image_config.rootfs.diff_ids.len() != manifest.layers.len()
        || image_config
            .rootfs
            .diff_ids
            .iter()
            .any(|digest| !valid_sha256(digest))
    {
        return Err(ProviderFailure::Archive);
    }
    for layer in &manifest.layers {
        verify_descriptor(layer)?;
        if layer.media_type != OCI_GZIP_LAYER_MEDIA_TYPE
            || layer.artifact_type.is_some()
            || layer.annotations.is_some()
            || layer.platform.is_some()
        {
            return Err(ProviderFailure::Archive);
        }
        verify_blob(archive, layer)?;
    }
    verify_runtime_config(&image_config.config, image)?;
    let config = serde_json::from_value(image_config.config.clone())
        .map_err(|_| ProviderFailure::Archive)?;
    Ok((
        config,
        image_config.rootfs.diff_ids,
        config_descriptor.digest.clone(),
    ))
}

fn verify_runtime_config(value: &Value, image: WorkerImage) -> Result<(), ProviderFailure> {
    let object = value.as_object().ok_or(ProviderFailure::Archive)?;
    let expected_fields = match image {
        WorkerImage::Runner => ["User", "Env", "Entrypoint", "WorkingDir", "Labels"].as_slice(),
        WorkerImage::Dind => ["Env", "Entrypoint", "Labels"].as_slice(),
    };
    if object.len() != expected_fields.len()
        || object
            .keys()
            .any(|field| !expected_fields.contains(&field.as_str()))
        || value.get("User").and_then(Value::as_str) != image.user()
        || value.get("WorkingDir").and_then(Value::as_str) != image.working_dir()
        || value
            .get("Env")
            .and_then(Value::as_array)
            .is_none_or(|env| {
                env.len() != 1 || env.first().and_then(Value::as_str) != Some(PATH_ENV)
            })
        || value
            .get("Entrypoint")
            .and_then(Value::as_array)
            .is_none_or(|entrypoint| {
                entrypoint.len() != 1
                    || entrypoint.first().and_then(Value::as_str) != Some(image.entrypoint())
            })
    {
        return Err(ProviderFailure::Archive);
    }
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    if labels.len() != BASE_LABELS.len()
        || labels
            .keys()
            .any(|label| !BASE_LABELS.contains(&label.as_str()))
        || labels
            .get("org.opencontainers.image.created")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.title")
            .and_then(Value::as_str)
            != Some("ubuntu")
        || labels
            .get("org.opencontainers.image.version")
            .and_then(Value::as_str)
            != Some("26.04")
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}

pub(super) fn verify_legacy_manifest(
    bytes: &[u8],
    image: WorkerImage,
    config_digest: &str,
    layers: &[Descriptor],
    expected_repo_tag: &str,
) -> Result<String, ProviderFailure> {
    let manifests: Vec<LegacyManifest> =
        serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let manifest = manifests.first().ok_or(ProviderFailure::Archive)?;
    let expected_config = blob_path(config_digest)?;
    let expected_layers = layers
        .iter()
        .map(|layer| blob_path(&layer.digest))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.len() != 1
        || manifest.config != expected_config
        || manifest.repo_tags.len() != 1
        || manifest.repo_tags.first().map(String::as_str) != Some(expected_repo_tag)
        || expected_repo_tag.split(':').next() != Some(image.image_name())
        || manifest.layers != expected_layers
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(expected_repo_tag.to_owned())
}

pub(super) fn select_children(
    descriptors: &[Descriptor],
) -> Result<(&Descriptor, &Descriptor), ProviderFailure> {
    let mut platform = None;
    let mut attestation = None;
    for descriptor in descriptors {
        match descriptor.platform.as_ref() {
            Some(value) if value.os == "linux" && value.architecture == "amd64" => {
                if platform.replace(descriptor).is_some()
                    || descriptor.annotations.is_some()
                    || descriptor.artifact_type.is_some()
                    || descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            Some(value) if value.os == "unknown" && value.architecture == "unknown" => {
                if attestation.replace(descriptor).is_some()
                    || descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE
                {
                    return Err(ProviderFailure::Archive);
                }
            }
            _ => return Err(ProviderFailure::Archive),
        }
        verify_descriptor(descriptor)?;
    }
    Ok((
        platform.ok_or(ProviderFailure::Archive)?,
        attestation.ok_or(ProviderFailure::Archive)?,
    ))
}
