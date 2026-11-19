// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::cli::WriteRequestArgs;
use crate::error::CiError;
use crate::write_request::{RequestParts, RunnerContext, build, internal};

const PAYLOAD_LIMIT: u64 = 8 << 20;
const REQUEST_FILE: &str = "velnor/request/plan-v1-request.json";

fn pick(argument: Option<&str>, variable: &str) -> Option<String> {
    argument
        .map(str::to_owned)
        .or_else(|| std::env::var(variable).ok())
}

pub(super) fn run(args: &WriteRequestArgs) -> Result<String, CiError> {
    let anchor = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| anchor.join(REQUEST_FILE));
    let event_name = pick(args.event_name.as_deref(), "GITHUB_EVENT_NAME").unwrap_or_default();
    let payload_path = args
        .event_path
        .clone()
        .or_else(|| {
            std::env::var_os("GITHUB_EVENT_PATH")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .ok_or_else(|| internal("missing_event_payload"))?;
    let payload = read_payload(&payload_path)?;
    let sha = pick(args.sha.as_deref(), "GITHUB_SHA").filter(|sha| !sha.is_empty());
    let repository =
        pick(args.repository.as_deref(), "GITHUB_REPOSITORY").filter(|slug| !slug.is_empty());
    let git_ref = pick(args.git_ref.as_deref(), "GITHUB_REF");
    let ref_protected = pick(args.ref_protected.as_deref(), "GITHUB_REF_PROTECTED");
    let workflow_ref = pick(args.workflow_ref.as_deref(), "GITHUB_WORKFLOW_REF");
    let workflow_sha = pick(args.workflow_sha.as_deref(), "GITHUB_WORKFLOW_SHA");
    let run_id = pick(args.run_id.as_deref(), "GITHUB_RUN_ID");
    let run_attempt = pick(args.run_attempt.as_deref(), "GITHUB_RUN_ATTEMPT");
    let bytes = build(&RequestParts {
        event_name: &event_name,
        payload_json: &payload,
        github_sha: sha.as_deref(),
        repository: repository.as_deref(),
        runner: RunnerContext {
            repository: repository.as_deref(),
            git_ref: git_ref.as_deref(),
            ref_protected: ref_protected.as_deref(),
            workflow_ref: workflow_ref.as_deref(),
            workflow_sha: workflow_sha.as_deref(),
            source_sha: sha.as_deref(),
            run_id: run_id.as_deref(),
            run_attempt: run_attempt.as_deref(),
        },
        named_check_lanes: args.named_check_lanes.as_deref(),
        lane_inventory: args.lane_inventory.as_deref(),
    })?;
    write_once(&anchor, &output, &bytes, "request")?;
    Ok(String::new())
}

fn read_payload(path: &Path) -> Result<String, CiError> {
    let unreadable = || internal("unreadable_event_payload");
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| unreadable())?;
    if !file.metadata().map_err(|_| unreadable())?.is_file() {
        return Err(unreadable());
    }
    let mut text = String::new();
    Read::take(file, PAYLOAD_LIMIT + 1)
        .read_to_string(&mut text)
        .map_err(|_| unreadable())?;
    if text.len() as u64 > PAYLOAD_LIMIT {
        return Err(unreadable());
    }
    Ok(text)
}

fn refuse_link(path: &Path) -> Result<(), CiError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(internal("symlink_refused"));
    }
    Ok(())
}

pub(super) fn write_once(
    anchor: &Path,
    path: &Path,
    bytes: &[u8],
    context: &str,
) -> Result<(), CiError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let relative = parent
        .strip_prefix(anchor)
        .map_err(|_| internal("anchor_escape"))?;
    let mut current = anchor.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(internal("anchor_escape"));
        };
        current.push(part);
        refuse_link(&current)?;
        if !current.is_dir() {
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(CiError::new(format!(
                        "cannot create {}: {error}",
                        current.display()
                    )));
                }
            }
        }
        refuse_link(&current)?;
    }
    refuse_link(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            internal(&format!(
                "{context}{}",
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "_exists"
                } else {
                    "_unwritable"
                }
            ))
        })?;
    file.write_all(bytes)
        .map_err(|_| internal(&format!("{context}_unwritable")))
}
