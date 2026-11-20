// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::http::HttpGitHub;
use super::open_regular;
use super::write_request::write_once;
use crate::cli::ResolveQualificationArgs;
use crate::error::CiError;
use crate::resolve_qualification::{ADMISSION_FILE, Api, Request, admission};
use crate::write_request::{RunnerContext, dispatch_for_parts, internal};

const FILE_LIMIT: u64 = 8 << 20;
const API_LIMIT: u64 = 1 << 20;
const API_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_FILE: &str = "velnor/request/plan-v1-request.json";

struct Http(HttpGitHub);

impl Api for Http {
    fn get(&self, route: &str) -> Option<Vec<u8>> {
        self.0.get_bytes(&format!("/{route}"), API_LIMIT)
    }
}

fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn non_empty(name: &str) -> Option<String> {
    variable(name).filter(|value| !value.is_empty())
}

/// The file the legacy op reads: not a symlink, a regular file, within the limit, UTF-8.
fn read_event_file(path: &Path) -> Result<String, CiError> {
    let unreadable = || internal("unreadable_event_payload");
    let file = open_regular(path).ok_or_else(unreadable)?;
    let mut bytes = Vec::new();
    Read::take(file, FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(unreadable());
    }
    String::from_utf8(bytes).map_err(|_| unreadable())
}

fn git(root: &Path, args: &[&str]) -> Option<(bool, String)> {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command.current_dir(root).args(args).output().ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

fn checkout_root() -> Result<PathBuf, CiError> {
    let cwd =
        std::env::current_dir().map_err(|error| internal(&format!("root_discovery: {error}")))?;
    let inside = git(&cwd, &["rev-parse", "--is-inside-work-tree"])
        .ok_or_else(|| internal("root_discovery: cannot run git"))?;
    if !inside.0 || inside.1.trim() != "true" {
        return Err(internal(&format!(
            "not_work_tree: not_inside_work_tree:{}",
            cwd.display()
        )));
    }
    let top = git(&cwd, &["rev-parse", "--show-toplevel"])
        .filter(|(success, _)| *success)
        .ok_or_else(|| internal("root_discovery: git rev-parse --show-toplevel failed"))?;
    Ok(PathBuf::from(top.1.trim()))
}

/// The legacy `is_valid_branch_name`.
fn valid_branch_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "HEAD"
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with(['.', '-'])
                && !part.ends_with('.')
                && part
                    .rsplit_once('.')
                    .is_none_or(|(_, suffix)| suffix != "lock")
        })
}

/// The default branch the request is checked against: the argument, else `origin/HEAD`, else the
/// event payload's `repository.default_branch`.
fn default_branch(argument: Option<&str>, root: &Path) -> Result<String, CiError> {
    if let Some(branch) = argument {
        return Ok(branch.to_owned());
    }
    if let Some((true, text)) = git(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        let text = text.trim();
        let branch = text.strip_prefix("origin/").unwrap_or(text);
        if valid_branch_name(branch) {
            return Ok(branch.to_owned());
        }
    }
    if let Some(path) = std::env::var_os("GITHUB_EVENT_PATH").filter(|path| !path.is_empty()) {
        let payload = read_event_file(Path::new(&path))?;
        let value: Value =
            serde_json::from_str(&payload).map_err(|_| internal("malformed_event_payload"))?;
        let branch = value["repository"]["default_branch"]
            .as_str()
            .filter(|branch| valid_branch_name(branch));
        if let Some(branch) = branch {
            return Ok(branch.to_owned());
        }
    }
    Err(internal(
        "default_branch: origin_head_unresolvable:set workflow.default_branch",
    ))
}

/// The dispatch the runner derives from its own `GITHUB_*` context must be the request's.
fn check_runner(request: &Request) -> Result<(), CiError> {
    let event_name = variable("GITHUB_EVENT_NAME").unwrap_or_default();
    if event_name != "workflow_dispatch" {
        return Err(internal("qualification_event_mismatch"));
    }
    let path = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_event_payload"))?;
    let payload: Value = serde_json::from_str(&read_event_file(&path)?)
        .map_err(|_| internal("malformed_event_payload"))?;
    let names = [
        "GITHUB_REPOSITORY",
        "GITHUB_REF",
        "GITHUB_REF_PROTECTED",
        "GITHUB_WORKFLOW_REF",
        "GITHUB_WORKFLOW_SHA",
        "GITHUB_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
    ];
    let [
        repository,
        git_ref,
        ref_protected,
        workflow_ref,
        workflow_sha,
        source_sha,
        run_id,
        run_attempt,
    ] = names.map(variable);
    let derived = dispatch_for_parts(
        &event_name,
        &payload,
        &RunnerContext {
            repository: repository.as_deref(),
            git_ref: git_ref.as_deref(),
            ref_protected: ref_protected.as_deref(),
            workflow_ref: workflow_ref.as_deref(),
            workflow_sha: workflow_sha.as_deref(),
            source_sha: source_sha.as_deref(),
            run_id: run_id.as_deref(),
            run_attempt: run_attempt.as_deref(),
        },
    )?;
    if request.matches_runner(derived.as_ref()) {
        Ok(())
    } else {
        Err(internal("qualification_runner_context_mismatch"))
    }
}

pub(super) fn run(args: &ResolveQualificationArgs) -> Result<String, CiError> {
    let runner_temp = non_empty("RUNNER_TEMP")
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let path = args
        .request
        .clone()
        .unwrap_or_else(|| runner_temp.join(REQUEST_FILE));
    if path != runner_temp.join(REQUEST_FILE) {
        return Err(internal("qualification_request_path_mismatch"));
    }
    let token = non_empty("GH_TOKEN")
        .or_else(|| non_empty("GITHUB_TOKEN"))
        .ok_or_else(|| internal("missing_github_token"))?;
    let request = Request::parse(&read_event_file(&path)?)?;
    let root = checkout_root()?;
    request.check()?;
    request.check_context(&default_branch(args.default_branch.as_deref(), &root)?)?;
    check_runner(&request)?;
    let api = Http(HttpGitHub::new(super::api_url(), token, API_TIMEOUT));
    let Some(bytes) = admission(&request, &root, &api)? else {
        return Ok(String::new());
    };
    let target = runner_temp.join(ADMISSION_FILE);
    write_once(&runner_temp, &target, &bytes, "qualification_admission")?;
    Ok(String::new())
}
