// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use base64::Engine as _;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn checkout_from_environment(mode: &str) -> Result<(), OrchestratorError> {
    let invalid = || internal("release_checkout_invalid");
    let required = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)
    };
    if std::env::consts::OS != "linux" || required("GITHUB_SERVER_URL")? != "https://github.com" {
        return Err(invalid());
    }
    let workspace = PathBuf::from(required("GITHUB_WORKSPACE")?);
    if !workspace.is_absolute() || workspace.canonicalize().map_err(|_| invalid())? != workspace {
        return Err(invalid());
    }
    let target = match mode {
        "policy" => workspace.clone(),
        "source" => {
            let path = workspace.join("release-source");
            fs::create_dir(&path).map_err(|_| invalid())?;
            path
        }
        _ => return Err(invalid()),
    };
    let sha = match mode {
        "policy" => required("GITHUB_WORKFLOW_SHA")?,
        "source" => required("VELNOR_SOURCE_SHA")?,
        _ => return Err(invalid()),
    };
    let repository = required("GITHUB_REPOSITORY")?;
    let token = required("GIT_TOKEN")?;
    checkout(
        &target,
        &repository,
        &sha,
        &token,
        "https://github.com",
        false,
    )
}

fn checkout(
    target: &Path,
    repository: &str,
    sha: &str,
    token: &str,
    origin: &str,
    test_http: bool,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("release_checkout_invalid");
    let (owner, name) = repository.split_once('/').ok_or_else(invalid)?;
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(name)
        || sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || token.is_empty()
        || token.len() > 8192
        || token.bytes().any(|b| b.is_ascii_control())
        || !target.is_absolute()
        || !target.is_dir()
        || target.canonicalize().map_err(|_| invalid())? != target
        || fs::read_dir(target)
            .map_err(|_| invalid())?
            .next()
            .is_some()
    {
        return Err(invalid());
    }
    let remote = format!("{origin}/{repository}.git");
    git(
        target,
        &["init".into(), "--quiet".into()],
        None,
        origin,
        test_http,
    )?;
    git(
        target,
        &[
            "remote".into(),
            "add".into(),
            "origin".into(),
            remote.into(),
        ],
        None,
        origin,
        test_http,
    )?;
    git(
        target,
        &[
            "fetch".into(),
            "--quiet".into(),
            "--no-recurse-submodules".into(),
            "--no-tags".into(),
            "origin".into(),
            "refs/heads/*:refs/remotes/origin/*".into(),
            "refs/tags/*:refs/tags/*".into(),
        ],
        Some(token),
        origin,
        test_http,
    )?;
    git(
        target,
        &[
            "checkout".into(),
            "--quiet".into(),
            "--detach".into(),
            sha.into(),
        ],
        None,
        origin,
        test_http,
    )?;
    if git(
        target,
        &["rev-parse".into(), "HEAD".into()],
        None,
        origin,
        test_http,
    )?
    .trim()
        != sha
        || !git(
            target,
            &[
                "status".into(),
                "--porcelain".into(),
                "--untracked-files=all".into(),
            ],
            None,
            origin,
            test_http,
        )?
        .is_empty()
        || git(
            target,
            &["rev-parse".into(), "--is-shallow-repository".into()],
            None,
            origin,
            test_http,
        )?
        .trim()
            != "false"
    {
        return Err(invalid());
    }
    Ok(())
}

fn git(
    target: &Path,
    args: &[OsString],
    token: Option<&str>,
    origin: &str,
    test_http: bool,
) -> Result<String, OrchestratorError> {
    let invalid = || internal("release_checkout_git_failed");
    let mut options = vec![
        ("core.hooksPath".to_owned(), "/dev/null".to_owned()),
        ("core.fsmonitor".to_owned(), "false".to_owned()),
        ("credential.helper".to_owned(), String::new()),
        ("gc.auto".to_owned(), "0".to_owned()),
        ("maintenance.auto".to_owned(), "false".to_owned()),
        ("fetch.recurseSubmodules".to_owned(), "false".to_owned()),
        ("http.followRedirects".to_owned(), "false".to_owned()),
        ("protocol.allow".to_owned(), "never".to_owned()),
        ("protocol.https.allow".to_owned(), "always".to_owned()),
    ];
    if test_http {
        options.push(("protocol.http.allow".to_owned(), "always".to_owned()));
    }
    if let Some(token) = token {
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
        options.push((
            format!("http.{origin}/.extraheader"),
            format!("Authorization: Basic {encoded}"),
        ));
    }
    let mut command = Command::new("/usr/bin/git");
    command
        .args(args)
        .current_dir(target)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", options.len().to_string());
    for (index, (key, value)) in options.into_iter().enumerate() {
        command
            .env(format!("GIT_CONFIG_KEY_{index}"), key)
            .env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
    let output = command.output().map_err(|_| invalid())?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return Err(invalid());
    }
    String::from_utf8(output.stdout).map_err(|_| invalid())
}

#[cfg(test)]
#[path = "crate_release_checkout_tests.rs"]
mod tests;
