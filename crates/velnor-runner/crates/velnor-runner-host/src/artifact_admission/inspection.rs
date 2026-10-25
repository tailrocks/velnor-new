//! Pure validation for Docker image inspection responses.

use bollard::models::{ContainerConfig, ImageInspect};

use crate::error::HostError;

use super::archive::ArchiveIdentity;
use super::manifest::{PLATFORM, ProbeManifest};

pub(super) fn validate_inspect(
    inspect: ImageInspect,
    manifest: &ProbeManifest,
    identity: &ArchiveIdentity,
) -> Result<String, HostError> {
    if inspect.os.as_deref() != Some("linux")
        || inspect.architecture.as_deref() != Some("amd64")
        || inspect.variant.is_some()
    {
        return Err(HostError::Identity);
    }
    let runtime_id = inspect.id.ok_or(HostError::Identity)?;
    if !super::hash::is_sha256_digest(&runtime_id) {
        return Err(HostError::Identity);
    }
    match inspect.descriptor {
        Some(descriptor) => {
            let size =
                i64::try_from(identity.image_manifest_size).map_err(|_| HostError::Identity)?;
            if descriptor.media_type.as_deref() != Some(identity.image_manifest_media_type.as_str())
                || descriptor.digest.as_deref() != Some(identity.image_manifest_digest.as_str())
                || descriptor.size.is_some_and(|value| value != size)
                || runtime_id != identity.image_manifest_digest
            {
                return Err(HostError::Identity);
            }
            if let Some(platform) = descriptor.platform
                && (platform.os.as_deref() != Some("linux")
                    || platform.architecture.as_deref() != Some("amd64")
                    || platform.variant.is_some()
                    || platform.os_version.is_some()
                    || platform.os_features.is_some())
            {
                return Err(HostError::Identity);
            }
        }
        None if runtime_id != identity.config_digest => return Err(HostError::Identity),
        None => {}
    }
    let config = inspect.config.ok_or(HostError::Identity)?;
    let value = serde_json::to_value(config).map_err(|_| HostError::Identity)?;
    let config: ContainerConfig = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    if config != identity.config || manifest.platform != PLATFORM {
        return Err(HostError::Identity);
    }
    Ok(runtime_id)
}
