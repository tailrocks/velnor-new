// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{archive_bytes, digest, read, write};
use crate::{OrchestratorError, internal::internal};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use velnor_actions_contract::preview_production::ReleaseProductionRequest;

#[derive(Debug)]
pub(super) struct QualifiedBinary {
    pub(super) path: PathBuf,
    _directory: tempfile::TempDir,
}

pub(super) fn build_qualified(
    request: &ReleaseProductionRequest,
    product: &str,
    scratch_root: &Path,
) -> Result<QualifiedBinary, OrchestratorError> {
    let root = verify_source(request)?;
    let target = scratch_root.join("sdk-preview-build");
    let stream = build_artifacts(request, &target)?;
    let artifacts = parse_artifacts(&stream)?;
    qualify(&root, request, product, &artifacts)
}

fn verify_source(request: &ReleaseProductionRequest) -> Result<PathBuf, OrchestratorError> {
    let root = std::env::current_dir().map_err(|_| internal("sdk_current_directory"))?;
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|_| internal("sdk_source_git"))?;
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .map_err(|_| internal("sdk_source_status"))?;
    if !head.status.success()
        || head.stdout != format!("{}\n", request.context.source_sha).as_bytes()
        || !status.status.success()
        || !status.stdout.is_empty()
    {
        return Err(internal("sdk_source_identity"));
    }
    Ok(root)
}

fn build_artifacts(
    request: &ReleaseProductionRequest,
    target: &Path,
) -> Result<Vec<u8>, OrchestratorError> {
    let output = std::process::Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--release",
            "--message-format=json",
            "--target",
            "x86_64-unknown-linux-gnu",
            "-p",
            "velnor-actions-cli",
            "-p",
            "velnor-archive-guard",
            "--bins",
        ])
        .env("CARGO_TARGET_DIR", target)
        .env("SDK_PREVIEW_VERSION_OVERRIDE", &request.context.version)
        .output()
        .map_err(|_| internal("sdk_cargo_spawn"))?;
    if !output.status.success() {
        std::io::stderr()
            .write_all(&output.stderr)
            .map_err(|_| internal("sdk_build_stderr"))?;
        return Err(internal("sdk_cargo_failed"));
    }
    let evidence = target.join("evidence");
    fs::create_dir(&evidence).map_err(|_| internal("sdk_build_evidence_directory"))?;
    let artifacts_path = evidence.join("artifacts.jsonl");
    write(&artifacts_path, &output.stdout)?;
    let metadata = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--format-version=1",
            "--filter-platform",
            "x86_64-unknown-linux-gnu",
        ])
        .env("CARGO_TARGET_DIR", target)
        .output()
        .map_err(|_| internal("sdk_metadata_spawn"))?;
    if !metadata.status.success() {
        return Err(internal("sdk_metadata_failed"));
    }
    let metadata_path = evidence.join("metadata.json");
    write(&metadata_path, &metadata.stdout)?;
    Ok(output.stdout)
}

fn parse_artifacts(stream: &[u8]) -> Result<BTreeMap<String, PathBuf>, OrchestratorError> {
    let mut artifacts = BTreeMap::new();
    for line in stream
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| internal("sdk_cargo_json"))?;
        if value["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(name) = value["target"]["name"].as_str() else {
            continue;
        };
        if !matches!(
            name,
            "velnor-actions" | "velnor-archive-guard" | "velnor-golden-collector"
        ) || value["target"]["kind"] != serde_json::json!(["bin"])
        {
            continue;
        }
        let executable = value["executable"]
            .as_str()
            .ok_or_else(|| internal("sdk_cargo_executable"))?;
        if artifacts
            .insert(name.to_owned(), PathBuf::from(executable))
            .is_some()
        {
            return Err(internal("sdk_cargo_duplicate"));
        }
    }
    if artifacts.len() != 3 {
        return Err(internal("sdk_cargo_inventory"));
    }
    Ok(artifacts)
}

fn qualify(
    root: &Path,
    request: &ReleaseProductionRequest,
    product: &str,
    artifacts: &BTreeMap<String, PathBuf>,
) -> Result<QualifiedBinary, OrchestratorError> {
    let scratch = tempfile::tempdir().map_err(|_| internal("sdk_qualification_directory"))?;
    let mut extracted = std::collections::BTreeMap::new();
    for (name, path) in artifacts {
        let bytes = read(path, 256 * 1024 * 1024)?;
        let archive = archive_bytes(name, &bytes)?;
        let members = crate::package_release_artifacts::extract_tar_gz_members(
            &archive,
            &std::collections::BTreeSet::from([name.clone()]),
            std::time::Instant::now(),
        )?;
        let extracted_bytes = members
            .get(name)
            .ok_or_else(|| internal("sdk_archive_member_missing"))?;
        if extracted_bytes != &bytes {
            return Err(internal("sdk_archive_member_bytes"));
        }
        let destination = scratch.path().join(name);
        write(&destination, extracted_bytes)?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
            .map_err(|_| internal("sdk_binary_mode"))?;
        extracted.insert(name.clone(), destination);
    }
    let cli = extracted
        .get("velnor-actions")
        .ok_or_else(|| internal("sdk_cli"))?;
    let guard = extracted
        .get("velnor-archive-guard")
        .ok_or_else(|| internal("sdk_guard"))?;
    let version = std::process::Command::new(cli)
        .arg("--version")
        .output()
        .map_err(|_| internal("sdk_version_spawn"))?;
    if !version.status.success()
        || version.stdout != format!("velnor-actions {}\n", request.plan.base_version).as_bytes()
    {
        return Err(internal("sdk_binary_version"));
    }
    let guard_digest = digest(&read(guard, 256 * 1024 * 1024)?);
    let oracle = scratch.path().join("oracle");
    let collector = extracted
        .get("velnor-golden-collector")
        .ok_or_else(|| internal("sdk_golden_collector"))?;
    for mode in ["capture", "check"] {
        let status = std::process::Command::new(collector)
            .arg(mode)
            .arg("--root")
            .arg(root)
            .arg("--cli")
            .arg(cli)
            .arg("--archive-guard")
            .arg(guard)
            .arg("--archive-guard-sha256")
            .arg(&guard_digest)
            .env("VELNOR_GOLDEN_DIR", &oracle)
            .status()
            .map_err(|_| internal("sdk_golden_spawn"))?;
        if !status.success() {
            return Err(internal("sdk_golden_failed"));
        }
    }
    for (name, path) in artifacts {
        if read(path, 256 * 1024 * 1024)?
            != read(
                extracted
                    .get(name)
                    .ok_or_else(|| internal("sdk_extracted_missing"))?,
                256 * 1024 * 1024,
            )?
        {
            return Err(internal("sdk_artifact_changed"));
        }
    }
    let path = extracted
        .remove(product)
        .ok_or_else(|| internal("sdk_product_binary"))?;
    Ok(QualifiedBinary {
        path,
        _directory: scratch,
    })
}
