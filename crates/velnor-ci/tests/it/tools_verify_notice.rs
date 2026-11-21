// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::tools_verify::{Job, discarded, job, recorded, rustc};

fn check_stdout(job: &Job, anchor: &str, env: &[(&str, &str)]) -> String {
    let event = job.root.join("event.json");
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#).unwrap_or_default();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
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

#[test]
fn a_poisoned_restore_names_its_cache_entry_and_the_command_that_deletes_it() {
    let job = job("tools-verify-named-key");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(stdout.contains("::notice::"), "{stdout}");
    assert!(stdout.contains("velnor-tools-abc123"), "{stdout}");
    assert!(
        stdout.contains("gh cache delete 'velnor-tools-abc123' -R tailrocks/velnor-new"),
        "{stdout}"
    );
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
        stdout.contains("gh cache delete 'velnor-tools-anchor-abc123' -R tailrocks/velnor-new"),
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
        stdout.contains("gh cache delete 'velnor-tools-anchor-abc123' -R tailrocks/velnor-new"),
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
        stdout.contains("gh cache delete 'velnor-tools-anchor-abc123' -R tailrocks/velnor-new"),
        "{stdout}"
    );
    assert!(discarded(&job));
}

#[test]
fn the_delete_command_states_that_it_removes_every_entry_with_the_key() {
    let job = job("tools-verify-named-scope");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(
        stdout.contains("removes every entry with that key in the repository, on any ref"),
        "{stdout}"
    );
}

#[test]
fn a_restore_on_github_enterprise_names_the_host_in_the_delete_command() {
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
        stdout.contains(
            "gh cache delete 'velnor-tools-abc123' -R ghe.example.com/tailrocks/velnor-new"
        ),
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
    assert!(!stdout.contains("gh cache delete"), "{stdout}");
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
        stdout.contains("gh cache delete 'velnor-tools-50%25%0D%0A::error::forged' -R"),
        "{stdout}"
    );
    assert!(!stdout.contains('\r'), "{stdout:?}");
}

#[test]
fn a_malformed_anchor_does_not_name_the_tools_entry() {
    let job = job("tools-verify-bad-anchor-tools-key");
    recorded(&job);
    let stdout = check_stdout(
        &job,
        "not-a-digest",
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("VELNOR_ANCHOR_CACHE_KEY", "velnor-tools-anchor-abc123"),
        ],
    );
    assert!(stdout.contains("the anchor is malformed"), "{stdout}");
    assert!(
        stdout.contains("gh cache delete 'velnor-tools-anchor-abc123' -R tailrocks/velnor-new"),
        "{stdout}"
    );
    assert!(!stdout.contains("velnor-tools-abc123'"), "{stdout}");
    assert!(discarded(&job));
}

#[test]
fn a_restore_on_github_com_names_the_repository_without_a_host() {
    let job = job("tools-verify-named-github-com");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123"),
            ("GITHUB_SERVER_URL", "https://github.com"),
        ],
    );
    assert!(
        stdout.contains("gh cache delete 'velnor-tools-abc123' -R tailrocks/velnor-new"),
        "{stdout}"
    );
    assert!(!stdout.contains("github.com/tailrocks"), "{stdout}");
}
