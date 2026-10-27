// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{ffi::OsString, path::Path};
use velnor_actions_mise::{CheckDeadline, GitRequest, IsolatedCommand};

const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 1024 * 1024;

pub(super) fn capture(
    repository: &str,
    base: &str,
    head: &str,
    landed: &str,
    pull_request: u64,
    deadline: CheckDeadline,
) -> Result<tempfile::TempDir, OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    if repository != transition.transition().tap_repository
        || pull_request == 0
        || ![base, head, landed]
            .into_iter()
            .all(super::history::valid_sha)
    {
        return Err(internal("homebrew_cutover_capture_selection"));
    }
    let staging = tempfile::Builder::new()
        .prefix("velnor-cutover-replay-")
        .tempdir()
        .map_err(|_| internal("homebrew_cutover_capture_create"))?;
    let root = staging.path();
    run(root, &["init", "--quiet"], deadline)?;
    let remote = format!("https://github.com/{repository}.git");
    let pull_ref = format!("refs/pull/{pull_request}/head:refs/remotes/velnor/cutover");
    run(
        root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-recurse-submodules",
            &remote,
            "refs/heads/main:refs/remotes/origin/main",
            &pull_ref,
        ],
        deadline,
    )?;
    if value(root, &["--is-shallow-repository"], deadline)? != "false"
        || object_bytes(&root.join(".git/objects"))? > MAX_OBJECT_BYTES
    {
        return Err(internal("homebrew_cutover_capture_incomplete"));
    }
    if value(
        root,
        &["--verify", "refs/remotes/velnor/cutover^{commit}"],
        deadline,
    )? != head
    {
        return Err(internal("homebrew_cutover_capture_head_changed"));
    }
    for sha in [base, head, landed, &transition.transition().base_sha] {
        if value(root, &["--verify", &format!("{sha}^{{commit}}")], deadline)? != sha {
            return Err(internal("homebrew_cutover_capture_commit"));
        }
    }
    require_ancestor(root, &transition.transition().base_sha, base, deadline)?;
    require_ancestor(root, base, landed, deadline)?;
    require_ancestor(root, landed, "refs/remotes/origin/main", deadline)?;
    validate_tree_bytes(root, base, deadline)?;
    validate_tree_bytes(root, head, deadline)?;
    run(root, &["checkout", "--quiet", "--detach", base], deadline)?;
    Ok(staging)
}

fn validate_tree_bytes(
    root: &Path,
    commit: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let entries = velnor_actions_mise::read_tap_tree(root, commit, deadline)
        .map_err(|_| internal("homebrew_cutover_capture_tree"))?;
    if entries.len() > 10_000 {
        return Err(internal("homebrew_cutover_capture_tree_limit"));
    }
    let mut total = 0usize;
    for entry in entries {
        let bytes =
            crate::homebrew::adoption::read_commit_file(root, commit, &entry.path, deadline)?;
        total = total
            .checked_add(bytes.len())
            .filter(|size| *size <= 16 * 1024 * 1024)
            .ok_or_else(|| internal("homebrew_cutover_capture_tree_limit"))?;
    }
    Ok(())
}

fn run(root: &Path, words: &[&str], deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let mut args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "protocol.file.allow=never",
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "submodule.recurse=false",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "init.templateDir=/dev/null",
        "-c",
        "http.followRedirects=false",
        "-c",
        "http.proxy=",
    ]
    .map(OsString::from)
    .to_vec();
    args.extend(words.iter().map(OsString::from));
    let env = git_env(root);
    let command = IsolatedCommand::repo_task("/usr/bin/git", args, &env)
        .map_err(|_| internal("homebrew_cutover_capture_command"))?
        .with_cwd(root.to_path_buf());
    let (output, _drained) = command
        .run_quiescent_until(OUTPUT_LIMIT, deadline)
        .map_err(|_| internal("homebrew_cutover_capture_failed"))?;
    output
        .require_success("git")
        .map(|_| ())
        .map_err(|_| internal("homebrew_cutover_capture_failed"))
}

fn git_env(root: &Path) -> Vec<(OsString, OsString)> {
    [
        ("HOME", root.as_os_str().to_owned()),
        ("XDG_CONFIG_HOME", root.as_os_str().to_owned()),
        ("PATH", OsString::from("/usr/bin:/bin")),
        ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
        ("GIT_CONFIG_SYSTEM", OsString::from("/dev/null")),
        ("GIT_CONFIG_GLOBAL", OsString::from("/dev/null")),
        ("GIT_TERMINAL_PROMPT", OsString::from("0")),
        ("GIT_ALLOW_PROTOCOL", OsString::from("https")),
        ("HTTP_PROXY", OsString::new()),
        ("HTTPS_PROXY", OsString::new()),
        ("ALL_PROXY", OsString::new()),
        ("http_proxy", OsString::new()),
        ("https_proxy", OsString::new()),
        ("all_proxy", OsString::new()),
        ("NO_PROXY", OsString::from("*")),
        ("no_proxy", OsString::from("*")),
    ]
    .map(|(key, value)| (OsString::from(key), value))
    .to_vec()
}

fn value(root: &Path, args: &[&str], deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let output = GitRequest::rev_parse(args.iter().map(OsString::from).collect())
        .command_in(root)
        .with_env(&git_env(root))
        .and_then(|command| command.run_until(OUTPUT_LIMIT, deadline))
        .map_err(|_| internal("homebrew_cutover_capture_commit"))?;
    output
        .require_success("git")
        .map_err(|_| internal("homebrew_cutover_capture_commit"))?;
    output
        .stdout_text("git")
        .map(|value| value.trim().to_owned())
        .map_err(|_| internal("homebrew_cutover_capture_commit"))
}

fn require_ancestor(
    root: &Path,
    base: &str,
    head: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let output = GitRequest::merge_base(["--is-ancestor", base, head].map(OsString::from).to_vec())
        .command_in(root)
        .with_env(&git_env(root))
        .and_then(|command| command.run_until(OUTPUT_LIMIT, deadline))
        .map_err(|_| internal("homebrew_cutover_capture_ancestry"))?;
    output
        .require_success("git")
        .map(|_| ())
        .map_err(|_| internal("homebrew_cutover_capture_ancestry"))
}

fn object_bytes(path: &Path) -> Result<u64, OrchestratorError> {
    let entries =
        std::fs::read_dir(path).map_err(|_| internal("homebrew_cutover_capture_objects"))?;
    let mut total = 0u64;
    for entry in entries {
        let path = entry
            .map_err(|_| internal("homebrew_cutover_capture_objects"))?
            .path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| internal("homebrew_cutover_capture_objects"))?;
        let bytes = if metadata.is_file() {
            metadata.len()
        } else if metadata.is_dir() {
            object_bytes(&path)?
        } else {
            return Err(internal("homebrew_cutover_capture_objects"));
        };
        total = total
            .checked_add(bytes)
            .filter(|size| *size <= MAX_OBJECT_BYTES)
            .ok_or_else(|| internal("homebrew_cutover_capture_objects_limit"))?;
    }
    Ok(total)
}
