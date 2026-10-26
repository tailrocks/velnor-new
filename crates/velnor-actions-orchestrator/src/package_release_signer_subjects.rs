// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OUTPUT_DIR, OrchestratorError, VerifiedSignedPackage, absolute_env, internal};
use std::io::Write as _;
use std::path::Path;
use velnor_actions_contract::package_release::PackageReleaseAssetKind;

pub(super) fn append_attestation_subject_paths(
    signed: &VerifiedSignedPackage,
    root: &Path,
) -> Result<(), OrchestratorError> {
    let runner_temp = absolute_env("RUNNER_TEMP")?;
    if !runner_temp.is_absolute() || root != runner_temp.join("velnor") {
        return Err(internal("package_signer_subject_root_invalid"));
    }
    let subject_dir = root.join(OUTPUT_DIR);
    if signed.directory() != subject_dir {
        return Err(internal("package_signer_subject_root_invalid"));
    }
    let mut paths = signed
        .receipt()
        .assets
        .iter()
        .filter(|asset| {
            matches!(
                asset.kind,
                PackageReleaseAssetKind::Payload | PackageReleaseAssetKind::Companion
            )
        })
        .map(|asset| subject_dir.join(&asset.name))
        .collect::<Vec<_>>();
    paths.push(subject_dir.join("release.json"));
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(internal("package_signer_subject_inventory_invalid"));
    }
    let mut output = String::from("attestation_subject_paths<<VELNOR_ATTESTATION_SUBJECT_PATHS\n");
    for path in paths {
        if path.parent() != Some(subject_dir.as_path())
            || !path.is_absolute()
            || path.to_string_lossy().contains(['\n', '\r'])
        {
            return Err(internal("package_signer_subject_path_invalid"));
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| internal("package_signer_subject_path_invalid"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Err(internal("package_signer_subject_path_invalid"));
        }
        output.push_str(&path.to_string_lossy());
        output.push('\n');
    }
    output.push_str("VELNOR_ATTESTATION_SUBJECT_PATHS\n");
    let output_path = absolute_env("GITHUB_OUTPUT")?;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(output_path)
        .map_err(|_| internal("package_signer_output_write"))?;
    file.write_all(output.as_bytes())
        .map_err(|_| internal("package_signer_output_write"))
}
