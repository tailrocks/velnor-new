//! Selected-daemon identity and verified-image checks for the probe provider.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::{ImageInspect, SystemInfo};
use sha2::{Digest, Sha256};

use crate::journal::{
    ImageImportDescriptor, ImageImportIdentity, ImageImportInspection, ImageImportObservation,
    ImageImportRequest,
};
use crate::launch::resource_probe::projection::DockerRoot;

use super::PLATFORM;
use super::release;
use super::{ProviderFailure, VerifiedCandidate, hex, valid_hex, valid_sha};

const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub(super) async fn verified_import_inspection(
    docker: &Docker,
    selected: &SelectedDaemon,
    release: &release::PreparedRelease,
    request: &ImageImportRequest,
    image: &ImageInspect,
) -> Result<ImageImportInspection, ProviderFailure> {
    verify_daemon_image(image, release)?;
    let observed = selected_daemon_info(docker, selected).await?;
    let observation = image_observation(image)?;
    ImageImportInspection::verified(
        request,
        &observed.engine_id,
        &observed.docker_root_digest,
        &observed.architecture,
        observation,
    )
    .map_err(|_| ProviderFailure::ImageIdentity)
}

pub(super) struct SelectedDaemon {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) architecture: String,
}

pub(super) fn selected_daemon(
    info: &SystemInfo,
    engine_id: &str,
) -> Result<SelectedDaemon, ProviderFailure> {
    if info.id.as_deref() != Some(engine_id) || !daemon_platform_matches(info) {
        return Err(ProviderFailure::ImageIdentity);
    }
    let root = info
        .docker_root_dir
        .as_deref()
        .ok_or(ProviderFailure::ImageIdentity)?;
    let root = DockerRoot::parse(root).map_err(|_| ProviderFailure::ImageIdentity)?;
    let architecture = daemon_architecture(info).ok_or(ProviderFailure::ImageIdentity)?;
    Ok(SelectedDaemon {
        engine_id: engine_id.to_owned(),
        docker_root_digest: root.digest().to_owned(),
        architecture: architecture.to_owned(),
    })
}

pub(super) fn image_import_request(
    selected: &SelectedDaemon,
    release: &release::PreparedRelease,
) -> Result<ImageImportRequest, ProviderFailure> {
    ImageImportRequest::new(ImageImportIdentity {
        engine_id: selected.engine_id.clone(),
        docker_root_digest: selected.docker_root_digest.clone(),
        daemon_architecture: selected.architecture.clone(),
        source_sha: release.source_revision.clone(),
        archive_sha256: release.archive_sha256.clone(),
        archive_index_digest: release.archive_index_digest.clone(),
        platform: PLATFORM.to_owned(),
        manifest_digest: release.manifest_digest.clone(),
        manifest_size: release.manifest_size,
        config_digest: release.config_digest.clone(),
        image_index_digest: None,
        image_index_size: None,
        load_archive_sha256: Some(release.load_archive_sha256.clone()),
        load_archive_index_digest: Some(release.load_archive_index_digest.clone()),
        image_reference: release.image_reference.clone(),
    })
    .map_err(|_| ProviderFailure::ImageImportJournal)
}

fn image_observation(image: &ImageInspect) -> Result<ImageImportObservation, ProviderFailure> {
    let image_id = image.id.clone().ok_or(ProviderFailure::ImageIdentity)?;
    let repo_tags = image.repo_tags.clone().unwrap_or_default();
    match image.descriptor.as_ref() {
        None if image.manifests.is_none() => Ok(ImageImportObservation::Classic {
            image_id,
            repo_tags,
        }),
        None => Err(ProviderFailure::ImageIdentity),
        Some(descriptor) => {
            let digest = descriptor
                .digest
                .clone()
                .ok_or(ProviderFailure::ImageIdentity)?;
            let media_type = descriptor
                .media_type
                .clone()
                .ok_or(ProviderFailure::ImageIdentity)?;
            let size = descriptor
                .size
                .and_then(|size| u64::try_from(size).ok())
                .ok_or(ProviderFailure::ImageIdentity)?;
            let annotations = descriptor
                .annotations
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            Ok(ImageImportObservation::Containerd {
                image_id,
                repo_tags,
                repo_digests: image.repo_digests.clone().unwrap_or_default(),
                descriptor: ImageImportDescriptor {
                    digest,
                    media_type,
                    size,
                    annotations,
                    has_extra_fields: descriptor.urls.is_some()
                        || descriptor.data.is_some()
                        || descriptor.platform.is_some()
                        || image.manifests.is_some(),
                },
            })
        }
    }
}

pub(super) async fn verify_selected_daemon(
    docker: &Docker,
    selected: &SelectedDaemon,
) -> Result<(), ProviderFailure> {
    selected_daemon_info(docker, selected).await.map(|_| ())
}

pub(super) async fn selected_daemon_info(
    docker: &Docker,
    selected: &SelectedDaemon,
) -> Result<SelectedDaemon, ProviderFailure> {
    let info = tokio::time::timeout(Duration::from_secs(20), docker.info())
        .await
        .map_err(|_| ProviderFailure::DaemonIdentity)?
        .map_err(|_| ProviderFailure::DaemonIdentity)?;
    let current = selected_daemon(&info, &selected.engine_id)?;
    if current.engine_id == selected.engine_id
        && current.docker_root_digest == selected.docker_root_digest
        && current.architecture == selected.architecture
    {
        Ok(current)
    } else {
        Err(ProviderFailure::ImageIdentity)
    }
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    info.os_type.as_deref() == Some("linux") && daemon_architecture(info).is_some()
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    match info.architecture.as_deref()? {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" => Some("aarch64"),
        _ => None,
    }
}

pub(super) fn compiled_source_revision() -> Result<&'static str, ProviderFailure> {
    let source_revision =
        option_env!("VELNOR_SOURCE_SHA").ok_or(ProviderFailure::TrustedSourcePinMissing)?;
    if valid_sha(source_revision) {
        Ok(source_revision)
    } else {
        Err(ProviderFailure::TrustedSourcePinInvalid)
    }
}

pub(super) fn build_pin_status() -> Result<(), &'static str> {
    compiled_source_revision()
        .map(|_| ())
        .map_err(ProviderFailure::code)
}

pub(super) async fn inspect_image(
    docker: &Docker,
    reference: &str,
) -> Result<Option<ImageInspect>, ProviderFailure> {
    match tokio::time::timeout(Duration::from_secs(20), docker.inspect_image(reference)).await {
        Ok(Ok(image)) => Ok(Some(image)),
        Ok(Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        })) => Ok(None),
        Ok(Err(_)) | Err(_) => Err(ProviderFailure::ImageInspect),
    }
}

pub(super) fn verify_daemon_image(
    image: &ImageInspect,
    release: &release::PreparedRelease,
) -> Result<(), ProviderFailure> {
    let id = image.id.as_deref().ok_or(ProviderFailure::ImageIdentity)?;
    if !digest_reference(id)
        || !store_identity_matches(image, id, release)
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
    {
        return Err(ProviderFailure::ImageIdentity);
    }
    let config = image.config.as_ref().ok_or(ProviderFailure::ImageConfig)?;
    let actual = serde_json::to_value(config).map_err(|_| ProviderFailure::ImageConfig)?;
    let expected =
        serde_json::to_value(&release.config).map_err(|_| ProviderFailure::ImageConfig)?;
    if actual != expected {
        return Err(ProviderFailure::ImageConfig);
    }
    Ok(())
}

fn store_identity_matches(
    image: &ImageInspect,
    id: &str,
    release: &release::PreparedRelease,
) -> bool {
    match image.descriptor.as_ref() {
        None => id == release.config_digest && image.manifests.is_none(),
        Some(descriptor) => {
            let Some((name, tag)) = release.image_reference.split_once(':') else {
                return false;
            };
            let expected_size = i64::try_from(release.manifest_size).ok();
            let expected_annotations = HashMap::from([
                (
                    "io.containerd.image.name".to_owned(),
                    format!("docker.io/library/{name}:{tag}"),
                ),
                (
                    "org.opencontainers.image.ref.name".to_owned(),
                    tag.to_owned(),
                ),
            ]);
            id == release.manifest_digest
                && descriptor.digest.as_deref() == Some(release.manifest_digest.as_str())
                && descriptor.media_type.as_deref() == Some(OCI_IMAGE_MANIFEST_MEDIA_TYPE)
                && descriptor.size == expected_size
                && descriptor.annotations.as_ref() == Some(&expected_annotations)
                && descriptor.urls.is_none()
                && descriptor.data.is_none()
                && descriptor.platform.is_none()
                && image.manifests.is_none()
        }
    }
}

pub(super) fn candidate(
    engine_id: &str,
    daemon_architecture: &str,
    image: ImageInspect,
    release: &release::PreparedRelease,
) -> Result<VerifiedCandidate, ProviderFailure> {
    verify_daemon_image(&image, release)?;
    let runtime_id = image.id.ok_or(ProviderFailure::ImageIdentity)?;
    let config = serde_json::from_value(
        serde_json::to_value(image.config.ok_or(ProviderFailure::ImageConfig)?)
            .map_err(|_| ProviderFailure::ImageConfig)?,
    )
    .map_err(|_| ProviderFailure::ImageConfig)?;
    let binding_fingerprint = image_binding(engine_id, daemon_architecture, &runtime_id, release);
    Ok(VerifiedCandidate {
        runtime_id,
        platform: PLATFORM.to_owned(),
        daemon_architecture: daemon_architecture.to_owned(),
        source_revision: release.source_revision.clone(),
        binding_fingerprint,
        config,
    })
}

fn image_binding(
    engine_id: &str,
    daemon_architecture: &str,
    runtime_id: &str,
    release: &release::PreparedRelease,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-resource-probe-image-binding-v1\0");
    for value in [
        engine_id,
        daemon_architecture,
        &release.source_revision,
        PLATFORM,
        &release.archive_sha256,
        &release.archive_index_digest,
        &release.manifest_digest,
        &release.config_digest,
        &release.load_archive_sha256,
        &release.load_archive_index_digest,
        &release.image_reference,
        runtime_id,
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

fn digest_reference(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex_value| valid_hex(hex_value, 64))
}
