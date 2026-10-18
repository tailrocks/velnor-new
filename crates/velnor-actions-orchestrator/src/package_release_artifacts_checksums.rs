//! Publisher-owned checksum sidecars derived from verified bytes.

use std::{io::Write, path::Path};

use sha2::{Digest, Sha256};
use velnor_actions_contract::package_release::{PackageReleaseAsset, PackageReleaseAssetKind};

use super::{VerifiedPackageArtifact, VerifiedPackageOutput, invalid};
use crate::OrchestratorError;

const HEX: &[u8; 16] = b"0123456789abcdef";

impl VerifiedPackageOutput {
    /// Create checksum sidecars and `SHA256SUMS` from independently computed
    /// archive digests. Producer-provided checksums are never read.
    pub(crate) fn write_checksum_assets(
        &self,
    ) -> Result<Vec<VerifiedPackageArtifact>, OrchestratorError> {
        let mut archives = self
            .artifacts
            .iter()
            .filter(|artifact| artifact.release_asset.kind == PackageReleaseAssetKind::Payload)
            .collect::<Vec<_>>();
        archives.sort_by(|left, right| left.name.cmp(&right.name));
        if archives.is_empty() {
            return Err(invalid("checksum_archive_inventory_empty"));
        }
        let parent = archives[0]
            .path
            .parent()
            .ok_or_else(|| invalid("checksum_staging_parent_missing"))?;
        if archives
            .iter()
            .any(|archive| archive.path.parent() != Some(parent))
        {
            return Err(invalid("checksum_staging_directory_mismatch"));
        }
        let mut sidecars = Vec::new();
        let mut sum_lines = Vec::new();
        for archive in archives {
            let sidecar_name = format!("{}.sha256", archive.name);
            let sidecar_path = parent.join(&sidecar_name);
            let sidecar_bytes = format!("{}  {}\n", archive.sha256, archive.name).into_bytes();
            write_generated_file(&sidecar_path, &sidecar_bytes)?;
            sidecars.push(VerifiedPackageArtifact {
                name: sidecar_name,
                path: sidecar_path,
                size: sidecar_bytes.len() as u64,
                sha256: sha256_bytes(&sidecar_bytes),
                release_asset: PackageReleaseAsset {
                    name: format!("{}.sha256", archive.name),
                    kind: PackageReleaseAssetKind::Sha256,
                    product_id: archive.release_asset.product_id.clone(),
                    target: archive.release_asset.target.clone(),
                    subject_name: Some(archive.name.clone()),
                    size: sidecar_bytes.len() as u64,
                    sha256: sha256_bytes(&sidecar_bytes),
                },
            });
            sum_lines.push(format!("{}  {}", archive.sha256, archive.name));
        }
        let sums_bytes = format!("{}\n", sum_lines.join("\n")).into_bytes();
        let sums_path = parent.join("SHA256SUMS");
        write_generated_file(&sums_path, &sums_bytes)?;
        sidecars.push(VerifiedPackageArtifact {
            name: "SHA256SUMS".to_owned(),
            path: sums_path,
            size: sums_bytes.len() as u64,
            sha256: sha256_bytes(&sums_bytes),
            release_asset: PackageReleaseAsset {
                name: "SHA256SUMS".to_owned(),
                kind: PackageReleaseAssetKind::ChecksumList,
                product_id: None,
                target: None,
                subject_name: None,
                size: sums_bytes.len() as u64,
                sha256: sha256_bytes(&sums_bytes),
            },
        });
        sidecars.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(sidecars)
    }
}

fn write_generated_file(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    file.write_all(bytes)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    file.sync_all()
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
