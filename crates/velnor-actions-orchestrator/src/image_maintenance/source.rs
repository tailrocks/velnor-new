// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use rustix::fs::{Mode, OFlags, open};
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    IMAGE_CANDIDATE_SOURCE_INPUTS, ImageCandidateSourceClosure, NATIVE_IMAGE_CANDIDATE_INPUTS,
    serialize_image_candidate_source_closure,
};
use velnor_actions_mise::{CheckDeadline, GitRequest};

use crate::{OrchestratorError, internal::internal};

use super::CheckJobSourceRequest;

const MAX_SOURCE_FILE_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn check_job_source(request: &CheckJobSourceRequest) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_candidate_deadline"))?;
    let root = checked_root(&request.root)?;
    let head = git_text(
        &root,
        &GitRequest::rev_parse(vec![OsString::from("HEAD")]),
        request.deadline,
    )?;
    if head.trim() != request.source_commit
        || env("GITHUB_SHA")? != request.source_commit
        || env("GITHUB_WORKFLOW_SHA")? != request.source_commit
        || !is_sha1(&request.source_commit)
    {
        return Err(internal("image_candidate_source_identity"));
    }
    if request.source_closure_path.starts_with(&root) {
        return Err(internal("image_candidate_closure_inside_source"));
    }
    ensure_clean_checkout(&root, request.deadline)?;
    let closure = source_closure(&root, &request.source_commit)?;
    write_new_file(
        &request.source_closure_path,
        &serialize_image_candidate_source_closure(&closure)
            .map_err(|_| internal("image_candidate_closure_serialize"))?,
    )?;
    Ok(())
}

pub(super) fn source_closure(
    root: &Path,
    source_commit: &str,
) -> Result<ImageCandidateSourceClosure, OrchestratorError> {
    let mut inputs = BTreeMap::new();
    for path in NATIVE_IMAGE_CANDIDATE_INPUTS {
        inputs.insert(path.to_owned(), hash_source_file(root, path)?);
    }
    ImageCandidateSourceClosure::new(source_commit.to_owned(), inputs)
        .map_err(|_| internal("image_candidate_closure_invalid"))
}

pub(super) fn source_inputs(root: &Path) -> Result<BTreeMap<String, String>, OrchestratorError> {
    IMAGE_CANDIDATE_SOURCE_INPUTS
        .into_iter()
        .map(|path| Ok((path.to_owned(), hash_source_file(root, path)?)))
        .collect()
}

pub(super) fn checked_root(root: &Path) -> Result<PathBuf, OrchestratorError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| OrchestratorError::io(root.display().to_string(), error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(internal("image_candidate_root_unsafe"));
    }
    root.canonicalize()
        .map_err(|error| OrchestratorError::io(root.display().to_string(), error.to_string()))
}

pub(super) fn ensure_clean_checkout(
    root: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let tracked = GitRequest::diff(vec![
        "--quiet".into(),
        "--no-ext-diff".into(),
        "HEAD".into(),
        "--".into(),
    ]);
    let tracked_status = tracked
        .run_in_until(root, deadline)
        .map_err(|_| internal("image_candidate_git_status"))?;
    if !tracked_status.success {
        return Err(internal("image_candidate_tracked_changes"));
    }
    let cached = GitRequest::diff(vec![
        "--cached".into(),
        "--quiet".into(),
        "--no-ext-diff".into(),
        "HEAD".into(),
        "--".into(),
    ]);
    if !cached
        .run_in_until(root, deadline)
        .map_err(|_| internal("image_candidate_git_status"))?
        .success
    {
        return Err(internal("image_candidate_index_changes"));
    }
    let untracked = GitRequest::ls_files(vec!["--others".into(), "--exclude-standard".into()]);
    let output = untracked
        .run_in_until(root, deadline)
        .map_err(|_| internal("image_candidate_git_status"))?;
    if !output.success || !output.stdout.is_empty() {
        return Err(internal("image_candidate_untracked_files"));
    }
    Ok(())
}

pub(super) fn git_text(
    root: &Path,
    request: &GitRequest,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let output = request
        .run_in_until(root, deadline)
        .map_err(|_| internal("image_candidate_git"))?;
    output
        .require_success("git")
        .map_err(|_| internal("image_candidate_git"))?;
    output
        .stdout_text("git")
        .map_err(|_| internal("image_candidate_git_utf8"))
}

pub(super) fn hash_source_file(root: &Path, relative: &str) -> Result<String, OrchestratorError> {
    if relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(internal("image_candidate_source_path"));
    }
    let path = root.join(relative);
    let before = fs::symlink_metadata(&path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > MAX_SOURCE_FILE_BYTES
    {
        return Err(internal("image_candidate_source_file"));
    }
    let fd = open(
        &path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| internal("image_candidate_source_open"))?;
    let mut file = fs::File::from(fd);
    let opened = file
        .metadata()
        .map_err(|_| internal("image_candidate_source_stat"))?;
    if opened.dev() != before.dev() || opened.ino() != before.ino() || !opened.is_file() {
        return Err(internal("image_candidate_source_identity"));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("image_candidate_source_read"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .filter(|size| *size <= MAX_SOURCE_FILE_BYTES)
            .ok_or_else(|| internal("image_candidate_source_size"))?;
        hasher.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| internal("image_candidate_source_stat"))?;
    let path_after =
        fs::symlink_metadata(&path).map_err(|_| internal("image_candidate_source_stat"))?;
    if after.len() != size
        || after.len() != opened.len()
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || after.mtime() != opened.mtime()
        || after.mtime_nsec() != opened.mtime_nsec()
        || after.ctime() != opened.ctime()
        || after.ctime_nsec() != opened.ctime_nsec()
        || path_after.dev() != opened.dev()
        || path_after.ino() != opened.ino()
        || !path_after.is_file()
        || path_after.file_type().is_symlink()
    {
        return Err(internal("image_candidate_source_changed"));
    }
    Ok(hex(&hasher.finalize()))
}

pub(super) fn env(name: &str) -> Result<String, OrchestratorError> {
    std::env::var(name).map_err(|_| internal("image_candidate_workflow_environment"))
}

pub(super) fn is_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

pub(super) fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    if !path.is_absolute() {
        return Err(internal("image_candidate_output_path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| internal("image_candidate_output_parent"))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| OrchestratorError::io(parent.display().to_string(), error.to_string()))?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent
            .canonicalize()
            .map_err(|_| internal("image_candidate_output_parent"))?
            != parent
    {
        return Err(internal("image_candidate_output_parent"));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}
