//! This module checks release files and prepares a private directory with size limits.

use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use bollard::models::ContainerConfig;

use super::ProviderFailure;
use super::archive::worker::{DerivedLoadArchive, ParsedWorkerArchive, WorkerImage};
use super::workspace::{VerifiedWorkspaceCleanup, Workspace};
use crate::artifact_admission::NativeArtifactProvider;

mod manifest;
mod metadata;

pub(super) use manifest::ResourceManifest;

const PROBE_ARCHIVE: &str = "velnor-resource-probe-linux-amd64.tar";
const MANIFEST_FILE: &str = "RESOURCE_PROBE_MANIFEST.json";
const CHECKSUMS_FILE: &str = "SHA256SUMS";

pub(super) struct PreparedRelease {
    pub(super) load_archive: Vec<u8>,
    pub(super) source_revision: String,
    pub(super) archive_sha256: String,
    pub(super) archive_index_digest: String,
    pub(super) manifest_digest: String,
    pub(super) manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
    pub(super) load_archive_sha256: String,
    pub(super) load_archive_index_digest: String,
    pub(super) image_reference: String,
    pub(super) workspace_cleanup: VerifiedWorkspaceCleanup,
}

pub(super) struct PreparedWorkerArtifact {
    pub(super) image: WorkerImage,
    pub(super) original: ParsedWorkerArchive,
    pub(super) load: DerivedLoadArchive,
}

pub(super) async fn prepare_native(
    provider: &NativeArtifactProvider<'_>,
    source_revision: &str,
) -> Result<PreparedRelease, ProviderFailure> {
    let material = provider
        .verified_probe_material()
        .await
        .map_err(|_| ProviderFailure::NativeAdmission)?;
    if material.source_sha != source_revision {
        return Err(ProviderFailure::TrustedSourcePinInvalid);
    }
    let workspace = Workspace::create_native()?;
    write_private_asset(&workspace.assets().join(PROBE_ARCHIVE), &material.archive)?;
    write_private_asset(&workspace.assets().join(MANIFEST_FILE), &material.manifest)?;
    write_private_asset(
        &workspace.assets().join(CHECKSUMS_FILE),
        &material.checksums,
    )?;
    let manifest = manifest::parse_manifest(&material.manifest, source_revision)?;
    let parsed = super::archive::verify(&material.archive, &manifest, source_revision)?;
    let workspace_cleanup = workspace.remove_verified()?;
    Ok(PreparedRelease {
        load_archive: parsed.load_archive,
        source_revision: source_revision.to_owned(),
        archive_sha256: parsed.archive_sha256,
        archive_index_digest: parsed.archive_index_digest,
        manifest_digest: parsed.manifest_digest,
        manifest_size: parsed.manifest_size,
        config_digest: parsed.config_digest,
        config: parsed.config,
        load_archive_sha256: parsed.load_archive_sha256,
        load_archive_index_digest: parsed.load_archive_index_digest,
        image_reference: parsed.image_reference,
        workspace_cleanup,
    })
}

fn write_private_asset(path: &std::path::Path, bytes: &[u8]) -> Result<(), ProviderFailure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|_| ProviderFailure::WorkspaceCreate)?;
    file.write_all(bytes)
        .map_err(|_| ProviderFailure::WorkspaceCreate)?;
    file.sync_all()
        .map_err(|_| ProviderFailure::WorkspaceCreate)
}
