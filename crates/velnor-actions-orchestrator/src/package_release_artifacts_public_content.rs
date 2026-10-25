//! Revalidate retained public release bytes before granting archive custody.

use super::{ArtifactKind, VerifiedPackageArtifact, archive, expected_files, invalid};
use crate::OrchestratorError;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write as _,
    time::Instant,
};
use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseAssetKind, PackageReleaseReceipt},
};

/// Revalidate every authenticated public payload, SBOM, companion, and
/// checksum sidecar before a public-release custody is granted.
pub(crate) fn validate_authenticated_release_content(
    plan: &PackageReleasePlan,
    receipt: &PackageReleaseReceipt,
    files: &BTreeMap<String, Vec<u8>>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(), OrchestratorError> {
    let expected = expected_files(&plan.products, &plan.companions, "release-output.json")?;
    let required_names = expected
        .iter()
        .filter(|file| file.kind != ArtifactKind::Handoff)
        .map(|file| file.name.as_str())
        .collect::<BTreeSet<_>>();
    for name in &required_names {
        if !files.contains_key(*name) {
            return Err(invalid("public_release_primary_asset_missing"));
        }
    }

    validate_public_checksums(receipt, files)?;

    let staging = tempfile::tempdir().map_err(|error| {
        OrchestratorError::io("public-release-content-staging", error.to_string())
    })?;
    let mut artifacts = Vec::with_capacity(required_names.len());
    for file in expected
        .iter()
        .filter(|file| file.kind != ArtifactKind::Handoff)
    {
        deadline
            .remaining()
            .map_err(|_| invalid("package_public_verification_deadline"))?;
        let bytes = files
            .get(&file.name)
            .ok_or_else(|| invalid("public_release_primary_asset_missing"))?;
        let path = staging.path().join(&file.name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&path).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        output.write_all(bytes).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        output.sync_all().map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        let asset = receipt
            .assets
            .iter()
            .find(|asset| asset.name == file.name)
            .ok_or_else(|| invalid("public_release_primary_asset_missing"))?;
        artifacts.push(VerifiedPackageArtifact {
            name: file.name.clone(),
            path,
            size: u64::try_from(bytes.len()).map_err(|_| invalid("public_release_asset_size"))?,
            sha256: sha256_bytes(bytes),
            release_asset: asset.clone(),
        });
    }
    deadline
        .scope(|| {
            archive::validate_copied_content(
                &artifacts,
                &expected,
                &receipt.source.commit_sha,
                &receipt.source.source_ref,
                &receipt.version.value,
                Instant::now(),
            )
        })
        .map_err(|_| invalid("package_public_verification_deadline"))?
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _format_result = write!(output, "{byte:02x}");
    }
    output
}

fn validate_public_checksums(
    receipt: &PackageReleaseReceipt,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    let payloads = receipt
        .assets
        .iter()
        .filter(|asset| asset.kind == PackageReleaseAssetKind::Payload)
        .collect::<Vec<_>>();
    if payloads.is_empty() {
        return Err(invalid("public_release_payload_inventory_empty"));
    }
    let mut sorted_payloads = payloads.clone();
    sorted_payloads.sort_by(|left, right| left.name.cmp(&right.name));
    let mut checksum_lines = Vec::with_capacity(sorted_payloads.len());
    for payload in &sorted_payloads {
        let expected_line = format!("{}  {}\n", payload.sha256, payload.name);
        let sidecar_name = format!("{}.sha256", payload.name);
        let sidecar = receipt
            .assets
            .iter()
            .find(|asset| {
                asset.name == sidecar_name && asset.kind == PackageReleaseAssetKind::Sha256
            })
            .ok_or_else(|| invalid("public_release_checksum_sidecar_missing"))?;
        if sidecar.subject_name.as_deref() != Some(payload.name.as_str())
            || files.get(&sidecar_name).map(Vec::as_slice) != Some(expected_line.as_bytes())
        {
            return Err(invalid("public_release_checksum_sidecar_mismatch"));
        }
        checksum_lines.push(format!("{}  {}", payload.sha256, payload.name));
    }
    let sums = receipt
        .assets
        .iter()
        .find(|asset| asset.kind == PackageReleaseAssetKind::ChecksumList)
        .ok_or_else(|| invalid("public_release_checksum_list_missing"))?;
    let expected_sums = format!("{}\n", checksum_lines.join("\n"));
    if sums.name != "SHA256SUMS"
        || files.get(&sums.name).map(Vec::as_slice) != Some(expected_sums.as_bytes())
    {
        return Err(invalid("public_release_checksum_list_mismatch"));
    }

    Ok(())
}
