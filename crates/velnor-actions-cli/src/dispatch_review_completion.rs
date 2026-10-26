// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    let temp = PathBuf::from(env::var_os("RUNNER_TEMP")?);
    if !temp.is_absolute() || path != temp.join("velnor/review-completion-policy.json") {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    Some(path.to_owned())
}

pub(super) fn capture() -> ExitCode {
    let digest = match velnor_actions_orchestrator::capture_review_completion() {
        Ok(digest) => digest,
        Err(error) => return super::fail_internal(&error.to_string()),
    };
    let Some(path) = env::var_os("GITHUB_OUTPUT") else {
        return super::fail_internal("missing github output");
    };
    let eligible = digest.is_some();
    match fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| {
            writeln!(
                file,
                "eligible={eligible}\nproposal_digest={}",
                digest.unwrap_or_default()
            )
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}

pub(super) fn emit() -> ExitCode {
    match velnor_actions_orchestrator::emit_review_completion() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}
