//! Fixed source handoff and identity-checked native staging.

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
    io::Read,
    path::{Path, PathBuf},
};
use velnor_actions_contract::{
    canonical_json_bytes,
    preview_production::{PreviewPayloadPair, PreviewProductionRequest},
};
use velnor_actions_mise::CheckDeadline;

pub(super) const TASK_REQUEST: &str = "preview-task-request.json";
pub(super) const PRODUCTION_REQUEST: &str = "preview-production-request.json";
pub(crate) const LEAF_MANIFEST: &str = "preview-payload.json";
pub(crate) const AGGREGATE_MANIFEST: &str = "preview-aggregate.json";
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

pub(super) struct PinnedAsset {
    path: PathBuf,
    proof: AssetProof,
    file: fs::File,
    original: fs::Metadata,
}

impl PinnedAsset {
    pub(super) fn new(
        path: &Path,
        proof: &AssetProof,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        recheck(path, proof, deadline)?;
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("preview_asset_open"))?;
        let file = fs::File::from(fd);
        let original = file
            .metadata()
            .map_err(|_| internal("preview_asset_stat"))?;
        let pinned = Self {
            path: path.to_owned(),
            proof: proof.clone(),
            file,
            original,
        };
        pinned.verify(deadline)?;
        Ok(pinned)
    }

    pub(super) fn verify(&self, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
        self.verify_identity()?;
        recheck(&self.path, &self.proof, deadline)?;
        self.verify_identity()
    }

    fn verify_identity(&self) -> Result<(), OrchestratorError> {
        use std::os::unix::fs::MetadataExt;
        let now = self
            .file
            .metadata()
            .map_err(|_| internal("preview_asset_stat"))?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| internal("preview_asset_stat"))?;
        let before = &self.original;
        if !named.is_file()
            || before.dev() != named.dev()
            || before.ino() != named.ino()
            || before.len() != now.len()
            || before.mode() != now.mode()
            || before.uid() != now.uid()
            || before.gid() != now.gid()
            || before.uid() != named.uid()
            || before.gid() != named.gid()
            || before.nlink() != 1
            || now.nlink() != 1
            || named.nlink() != 1
            || before.mtime() != now.mtime()
            || before.mtime_nsec() != now.mtime_nsec()
            || before.ctime() != now.ctime()
            || before.ctime_nsec() != now.ctime_nsec()
        {
            return Err(internal("preview_admitted_input_replaced"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskInput {
    pub pair: PreviewPayloadPair,
    pub name: String,
    pub sha256: String,
    pub size: u64,
    pub path: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest<'a> {
    schema: &'static str,
    request: &'a PreviewProductionRequest,
    inputs: &'a [TaskInput],
}

pub(super) struct TaskHandoff {
    pub(super) path: PathBuf,
    pinned: Vec<PinnedAsset>,
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
    request: &PreviewProductionRequest,
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
        pinned.push(PinnedAsset::new(&temp.join(name), &proof, deadline)?);
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
    let mut pinned = PinnedAsset::new(path, proof, deadline)?;
    if pinned.original.uid() != crate::filesystem_owner::effective_uid()
        || pinned.original.nlink() != 1
    {
        return Err(internal("preview_upload_owner"));
    }
    rustix::fs::fchmod(
        &pinned.file,
        rustix::fs::Mode::RUSR
            | rustix::fs::Mode::WUSR
            | rustix::fs::Mode::RGRP
            | rustix::fs::Mode::ROTH,
    )
    .map_err(|_| internal("preview_upload_mode"))?;
    pinned.original = pinned
        .file
        .metadata()
        .map_err(|_| internal("preview_upload_stat"))?;
    if pinned.original.mode() & 0o7777 != 0o644
        || pinned.original.uid() != crate::filesystem_owner::effective_uid()
        || pinned.original.nlink() != 1
    {
        return Err(internal("preview_upload_mode"));
    }
    pinned.verify(deadline)
}

pub(super) fn recheck(
    path: &Path,
    proof: &AssetProof,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::MetadataExt;
    crate::check_evidence::reject_link_components(
        Path::new("/"),
        path.strip_prefix("/")
            .map_err(|_| internal("preview_asset_path"))?
            .to_str()
            .ok_or_else(|| internal("preview_asset_path"))?,
    )?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("preview_asset_open"))?;
    let mut file = fs::File::from(fd);
    let before = file
        .metadata()
        .map_err(|_| internal("preview_asset_stat"))?;
    if !before.is_file() || before.len() != proof.size {
        return Err(internal("preview_asset_identity_changed"));
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        checkpoint(deadline)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("preview_asset_read"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|size| *size <= proof.size)
            .ok_or_else(|| internal("preview_asset_identity_changed"))?;
        hash.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| internal("preview_asset_stat"))?;
    let named = fs::symlink_metadata(path).map_err(|_| internal("preview_asset_stat"))?;
    if total != proof.size
        || hex(&hash.finalize()) != proof.sha256
        || before.dev() != named.dev()
        || before.ino() != named.ino()
        || !named.is_file()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(internal("preview_asset_identity_changed"));
    }
    checkpoint(deadline)
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
