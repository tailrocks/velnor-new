// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::tools_verify::{Job, discarded, job, recorded, rustc};

const REPO: &str = "tailrocks/velnor-new";

pub(crate) fn check_stdout(job: &Job, anchor: &str, env: &[(&str, &str)]) -> String {
    let event = job.root.join("event.json");
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#).unwrap_or_default();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    let no_tools = job.root.join("no-tools");
    fs::create_dir_all(&no_tools).unwrap_or_default();
    command
        .env_clear()
        .current_dir(&job.root)
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

fn delete_entry(key: &str) -> String {
    format!("cache delete-entry --repo '{REPO}' --key '{key}' --ref 'refs/heads/main'")
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
        stdout.contains(&delete_entry("velnor-tools-abc123")),
        "{stdout}"
    );
    assert!(!stdout.contains("gh cache delete"), "{stdout}");
    assert!(discarded(&job));
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
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123")),
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
    assert!(stdout.contains("the anchor is malformed"), "{stdout}");
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123")),
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
    assert!(stdout.contains("the anchor is malformed"), "{stdout}");
    assert!(
        stdout.contains(&delete_entry("velnor-tools-anchor-abc123")),
        "{stdout}"
    );
    assert!(discarded(&job));
}

#[test]
fn the_notice_promises_the_first_ref_and_a_refusal_when_several_match() {
    let stdout = poisoned("tools-verify-named-scope", &[]);
    assert!(
        stdout.contains("deletes the entry on the first ref that holds the key"),
        "{stdout}"
    );
    assert!(stdout.contains("never touches a later one"), "{stdout}");
    assert!(
        stdout.contains("refuses unless exactly one entry has the key on that ref"),
        "{stdout}"
    );
    assert!(!stdout.contains("on any ref"), "{stdout}");
    assert!(!stdout.contains(" or `"), "{stdout}");
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
        stdout.contains("--key 'velnor-tools-50%25%0D%0A::error::forged' --ref"),
        "{stdout}"
    );
    assert!(!stdout.contains('\r'), "{stdout:?}");
}

fn refs_named(stdout: &str) -> Vec<String> {
    stdout
        .split("--ref '")
        .skip(1)
        .filter_map(|rest| rest.split_once('\'').map(|(name, _)| name.to_owned()))
        .collect()
}

fn poisoned(name: &str, env: &[(&str, &str)]) -> String {
    let job = job(name);
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let mut all = vec![("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")];
    all.extend_from_slice(env);
    check_stdout(&job, &anchor, &all)
}

#[test]
fn a_pull_request_run_names_its_merge_ref_and_the_default_branch() {
    let stdout = poisoned(
        "tools-verify-merge-ref",
        &[
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/7/merge"),
            ("GITHUB_BASE_REF", "main"),
        ],
    );
    assert_eq!(
        refs_named(&stdout),
        ["refs/pull/7/merge", "refs/heads/main"],
        "{stdout}"
    );
    assert_eq!(stdout.matches("cache delete-entry").count(), 1, "{stdout}");
}

#[test]
fn a_pull_request_into_another_branch_names_the_base_and_the_default_branch_too() {
    let stdout = poisoned(
        "tools-verify-base-ref",
        &[
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/7/merge"),
            ("GITHUB_BASE_REF", "release"),
        ],
    );
    assert_eq!(
        refs_named(&stdout),
        ["refs/pull/7/merge", "refs/heads/release", "refs/heads/main"],
        "{stdout}"
    );
}

#[test]
fn a_push_to_the_default_branch_names_that_ref_once() {
    let stdout = poisoned("tools-verify-default-ref", &[]);
    assert_eq!(refs_named(&stdout), ["refs/heads/main"], "{stdout}");
}

#[test]
fn the_notice_prints_no_version() {
    let stdout = poisoned("tools-verify-no-version", &[]);
    assert!(!stdout.contains("--version"), "{stdout}");
}

fn printed_command(stdout: &str) -> String {
    let (_, rest) = stdout
        .split_once('`')
        .unwrap_or_else(|| unreachable!("no command in {stdout}"));
    let (command, _) = rest
        .split_once('`')
        .unwrap_or_else(|| unreachable!("unterminated command in {stdout}"));
    command.to_owned()
}

fn run_printed_command(job: &Job, command: &str) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let bin = job.root.join("fake-bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    for (name, body) in [
        ("gh", "#!/bin/sh\necho the-token\n"),
        (
            "velnor-ci",
            "#!/bin/sh\nprintf '%s\\n' \"$GITHUB_API_URL\" \"$GITHUB_TOKEN\" \"$@\"\n",
        ),
    ] {
        let path = bin.join(name);
        fs::write(&path, body).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", command])
        .env_clear()
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_printed_command_quotes_a_hostile_key_and_base_branch_so_a_shell_reads_them_back_unchanged() {
    let job = job("tools-verify-shell-quoting");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let key = "velnor-tools-a'$(id)'b";
    let base = "a'$(id)'";
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", key),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/7/merge"),
            ("GITHUB_BASE_REF", base),
        ],
    );
    let read_back = run_printed_command(&job, &printed_command(&stdout));
    assert_eq!(
        read_back,
        [
            "https://api.github.com",
            "the-token",
            "cache",
            "delete-entry",
            "--repo",
            REPO,
            "--key",
            key,
            "--ref",
            "refs/pull/7/merge",
            "--ref",
            &format!("refs/heads/{base}"),
            "--ref",
            "refs/heads/main",
        ],
        "{stdout}"
    );
}
