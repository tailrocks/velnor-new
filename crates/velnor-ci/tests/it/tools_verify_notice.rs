// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use sha2::{Digest, Sha256};
use velnor_ci::image_candidate::hashing::hex;

use crate::tools_verify::{Job, discarded, job, recorded, rustc};

const GZIP_VERSION: &str = "0e860ad0f6378509df797cde101e51911acafca877e9ef5e166e86d508c549da";
const ZSTD_VERSION: &str = "0cccfb17094855db4c75b312ac9e0441b5c3cbdbccea182d94251e8142ff4640";
const REPO: &str = "tailrocks/velnor-new";

fn version(path: &str, compression: &str) -> String {
    let text = format!("{path}|{compression}|1.0");
    hex(&Sha256::digest(text.as_bytes()))
}

fn zstd_on_path(job: &Job) -> String {
    let bin = job.root.join("zstd-bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let zstd = bin.join("zstd");
    fs::write(&zstd, "#!/bin/sh\necho '*** Zstandard CLI v1.5.5'\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&zstd, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    bin.display().to_string()
}

fn check_stdout(job: &Job, anchor: &str, env: &[(&str, &str)]) -> String {
    let event = job.root.join("event.json");
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#).unwrap_or_default();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    let no_tools = job.root.join("no-tools");
    fs::create_dir_all(&no_tools).unwrap_or_default();
    command
        .env_clear()
        .env("PATH", &no_tools)
        .args(["tools-verify", "check"])
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", &job.root)
        .env("GITHUB_OUTPUT", job.root.join("github-output"))
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &event)
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("VELNOR_TOOLS_ANCHOR", anchor);
    for (name, value) in env {
        command.env(name, value);
    }
    let run = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(run.status.success(), "{run:?}");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn delete_entry(key: &str, version: &str) -> String {
    format!(
        "cache delete-entry --repo '{REPO}' --key '{key}' --version {version} --ref 'refs/heads/main'"
    )
}

#[test]
fn a_poisoned_restore_names_its_cache_entry_and_the_command_that_deletes_it_by_id() {
    let job = job("tools-verify-named-key");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(stdout.contains("::notice::"), "{stdout}");
    assert!(
        stdout.contains(&delete_entry("velnor-tools-abc123", GZIP_VERSION)),
        "{stdout}"
    );
    assert!(!stdout.contains("gh cache delete"), "{stdout}");
    assert!(discarded(&job));
}

#[test]
fn the_version_follows_the_compression_the_cache_action_picks_on_this_runner() {
    let job = job("tools-verify-named-zstd");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("PATH", &zstd_on_path(&job)),
        ],
    );
    assert!(
        stdout.contains(&delete_entry("velnor-tools-abc123", ZSTD_VERSION)),
        "{stdout}"
    );
}

#[test]
fn the_pinned_versions_are_what_the_cache_service_stores_for_the_tools_paths() {
    let paths = "~/.local/share/mise|~/.rustup";
    assert_eq!(GZIP_VERSION, version(paths, "gzip"));
    assert_eq!(ZSTD_VERSION, version(paths, "zstd-without-long"));
}

#[test]
fn a_poisoned_restore_names_the_anchor_entry_that_feeds_the_check() {
    let job = job("tools-verify-named-anchor");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("VELNOR_ANCHOR_CACHE_KEY", "velnor-tools-anchor-abc123"),
        ],
    );
    let anchor_version = version(
        &format!("{}/velnor-tools-anchor", job.root.display()),
        "gzip",
    );
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123", &anchor_version)),
        "{stdout}"
    );
}

#[test]
fn a_malformed_anchor_names_the_anchor_entry_that_feeds_the_check() {
    let job = job("tools-verify-bad-anchor");
    let stdout = check_stdout(
        &job,
        "not-a-digest",
        &[("VELNOR_ANCHOR_CACHE_KEY", "velnor-tools-anchor-abc123")],
    );
    let anchor_version = version(
        &format!("{}/velnor-tools-anchor", job.root.display()),
        "gzip",
    );
    assert!(stdout.contains("the anchor is malformed"), "{stdout}");
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123", &anchor_version)),
        "{stdout}"
    );
    assert!(discarded(&job));
}

#[test]
fn a_restored_malformed_anchor_names_the_anchor_entry_when_the_read_step_left_it_empty() {
    let job = job("tools-verify-restored-bad-anchor");
    let _ = recorded(&job);
    fs::write(&job.anchor, "velnor-tools-anchor-v1 not-a-digest\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        "",
        &[("VELNOR_ANCHOR_CACHE_KEY", "velnor-tools-anchor-abc123")],
    );
    let anchor_version = version(
        &format!("{}/velnor-tools-anchor", job.root.display()),
        "gzip",
    );
    assert!(stdout.contains("the anchor is malformed"), "{stdout}");
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123", &anchor_version)),
        "{stdout}"
    );
    assert!(discarded(&job));
}

#[test]
fn the_notice_promises_one_entry_and_a_refusal_when_several_match() {
    let job = job("tools-verify-named-scope");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(
        stdout.contains("deletes only the entry with that key and version"),
        "{stdout}"
    );
    assert!(
        stdout.contains("refuses when more than one entry matches"),
        "{stdout}"
    );
    assert!(!stdout.contains("on any ref"), "{stdout}");
}

#[test]
fn a_restore_on_github_enterprise_names_the_host_and_its_api() {
    let job = job("tools-verify-named-host");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("GITHUB_SERVER_URL", "https://ghe.example.com/"),
        ],
    );
    assert!(
        stdout.contains("GITHUB_API_URL='https://ghe.example.com/api/v3'"),
        "{stdout}"
    );
    assert!(
        stdout.contains("gh auth token --hostname 'ghe.example.com'"),
        "{stdout}"
    );
}

#[test]
fn the_runner_api_url_is_used_as_given() {
    let job = job("tools-verify-named-api");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("GITHUB_SERVER_URL", "https://ghe.example.com"),
            ("GITHUB_API_URL", "https://ghe.example.com/custom/api"),
        ],
    );
    assert!(
        stdout.contains("GITHUB_API_URL='https://ghe.example.com/custom/api'"),
        "{stdout}"
    );
}

#[test]
fn a_restore_without_a_server_url_names_github_com_so_gh_host_cannot_redirect_the_delete() {
    let job = job("tools-verify-named-github-com");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(
        stdout.contains("GITHUB_API_URL='https://api.github.com'"),
        "{stdout}"
    );
    assert!(
        stdout.contains("gh auth token --hostname 'github.com'"),
        "{stdout}"
    );
}

#[test]
fn a_restore_from_the_node_store_names_no_cache_entry() {
    let job = job("tools-verify-named-none");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(&job, &anchor, &[("VELNOR_TOOLS_CACHE_KEY", "")]);
    assert!(stdout.contains("discarded"), "{stdout}");
    assert!(!stdout.contains("delete-entry"), "{stdout}");
}

#[test]
fn a_hostile_cache_key_cannot_forge_a_second_workflow_command() {
    let job = job("tools-verify-hostile-key");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[(
            "VELNOR_TOOLS_CACHE_KEY",
            "velnor-tools-50%\r\n::error::forged",
        )],
    );
    assert_eq!(stdout.lines().count(), 1, "{stdout:?}");
    assert!(
        stdout.contains("--key 'velnor-tools-50%25%0D%0A::error::forged' --version"),
        "{stdout}"
    );
    assert!(!stdout.contains('\r'), "{stdout:?}");
}
