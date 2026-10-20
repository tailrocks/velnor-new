//! Fixed source handoff and identity-checked native staging.

use crate::filesystem_owner::held_file::HeldFile;
use crate::{
    OrchestratorError,
    exclusive_write::{create_dir_no_symlink, write_exclusive_until},
    internal::internal,
    package_release_artifacts::VerifiedPackageArtifact,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use velnor_actions_contract::{
    canonical_json_bytes,
    preview_production::{ReleasePayloadPair, ReleaseProductionRequest},
};
use velnor_actions_mise::CheckDeadline;

pub(super) const TASK_REQUEST: &str = "release-task-request.json";
pub(super) const PRODUCTION_REQUEST: &str = "release-production-request.json";
pub(crate) const LEAF_MANIFEST: &str = "release-payload.json";
pub(crate) const AGGREGATE_MANIFEST: &str = "release-aggregate.json";
pub(crate) const MAX_NATIVE_MANIFEST: u64 =
    crate::package_release_publish::artifact_zip::NATIVE_MANIFEST_BYTES;
#[path = "preview_production_handoff_copy.rs"]
mod copy;
pub(super) use copy::copy_verified;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetProof {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskInput {
    pub pair: ReleasePayloadPair,
    pub name: String,
    pub sha256: String,
    pub size: u64,
    pub path: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest<'a> {
    schema: &'static str,
    request: &'a ReleaseProductionRequest,
    inputs: &'a [TaskInput],
}

pub(super) struct TaskHandoff {
    pub(super) path: PathBuf,
    pinned: Vec<HeldFile>,
}

impl TaskHandoff {
    pub(super) fn verify(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        for file in &self.pinned {
            file.verify(deadline)?;
        }
        Ok(())
    }
}

pub(super) fn write_task_request(
    temp: &Path,
    request: &ReleaseProductionRequest,
    inputs: &[TaskInput],
    deadline: CheckDeadline,
) -> Result<TaskHandoff, OrchestratorError> {
    let production = request
        .canonical_bytes()
        .map_err(|_| internal("preview_request_encode"))?;
    write_exclusive_until(
        &temp.join(PRODUCTION_REQUEST),
        &production,
        "preview_production_request",
        || checkpoint(deadline),
    )?;
    let bytes = canonical_json_bytes(&TaskRequest {
        schema: "velnor.preview-task.v1",
        request,
        inputs,
    })
    .map_err(|_| internal("preview_task_request_encode"))?;
    if !u64::try_from(bytes.len()).is_ok_and(|size| size <= MAX_NATIVE_MANIFEST) {
        return Err(internal("preview_task_request_limit"));
    }
    let path = temp.join(TASK_REQUEST);
    write_exclusive_until(&path, &bytes, "preview_task_request", || {
        checkpoint(deadline)
    })?;
    let mut pinned = Vec::new();
    for (name, value) in [
        (PRODUCTION_REQUEST, production.as_slice()),
        (TASK_REQUEST, bytes.as_slice()),
    ] {
        let proof = AssetProof {
            name: name.to_owned(),
            sha256: sha256(value),
            size: value.len() as u64,
        };
        pinned.push(HeldFile::new(
            &temp.join(name),
            &proof.sha256,
            proof.size,
            deadline,
        )?);
    }
    Ok(TaskHandoff { path, pinned })
}

pub(super) fn fresh_directory(parent: &Path, name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = parent.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(internal("preview_directory_preexisting"));
    }
    create_dir_no_symlink(parent, &path)?;
    Ok(path)
}

pub(super) fn stage_asset(
    artifact: &VerifiedPackageArtifact,
    output: &Path,
    deadline: CheckDeadline,
) -> Result<AssetProof, OrchestratorError> {
    let proof = AssetProof {
        name: artifact.name.clone(),
        sha256: artifact.sha256.clone(),
        size: artifact.size,
    };
    validate_asset_name(&proof.name)?;
    let target = output.join(&proof.name);
    copy_verified(&artifact.path, &target, &proof, deadline)?;
    normalize_upload_file(&target, &proof, deadline)?;
    Ok(proof)
}

/// Only newly created upload files receive the literal mode expected in service ZIPs.
pub(super) fn write_upload(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    write_exclusive_until(path, bytes, "preview_upload", || checkpoint(deadline))?;
    let proof = AssetProof {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| internal("preview_upload_name"))?
            .to_owned(),
        sha256: sha256(bytes),
        size: bytes.len() as u64,
    };
    normalize_upload_file(path, &proof, deadline)
}

fn normalize_upload_file(
    path: &Path,
    proof: &AssetProof,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::MetadataExt;
    let pinned = HeldFile::new(path, &proof.sha256, proof.size, deadline)?;
    let original = pinned
        .file()
        .metadata()
        .map_err(|_| internal("preview_upload_stat"))?;
    if original.uid() != crate::filesystem_owner::effective_uid() || original.nlink() != 1 {
        return Err(internal("preview_upload_owner"));
    }
    rustix::fs::fchmod(
        pinned.file(),
        rustix::fs::Mode::RUSR
            | rustix::fs::Mode::WUSR
            | rustix::fs::Mode::RGRP
            | rustix::fs::Mode::ROTH,
    )
    .map_err(|_| internal("preview_upload_mode"))?;
    // Mode normalization is an explicit producer mutation, never a refreshed admission.
    let normalized = HeldFile::new(path, &proof.sha256, proof.size, deadline)?;
    let after = normalized
        .file()
        .metadata()
        .map_err(|_| internal("preview_upload_stat"))?;
    if after.mode() & 0o7777 != 0o644
        || after.dev() != original.dev()
        || after.ino() != original.ino()
        || after.uid() != original.uid()
        || after.gid() != original.gid()
    {
        return Err(internal("preview_upload_mode"));
    }
    drop(pinned);
    normalized.verify(deadline)
}

pub(super) fn recheck(
    path: &Path,
    proof: &AssetProof,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    HeldFile::new(path, &proof.sha256, proof.size, deadline).map(|_| ())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 15)]));
    }
    value
}

pub(crate) fn validate_asset_name(name: &str) -> Result<(), OrchestratorError> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(internal("preview_asset_name"));
    }
    Ok(())
}

pub(super) fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| internal("preview_deadline_exhausted"))
}

#[cfg(test)]
#[path = "preview_production_handoff_tests.rs"]
mod tests;
