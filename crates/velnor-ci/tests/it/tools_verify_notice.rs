// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::tools_verify::{Job, discarded, job, recorded, rustc};

fn check_stdout(job: &Job, key: Option<&str>) -> String {
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
        .env("VELNOR_ANCHOR_HIT", "true");
    if let Some(key) = key {
        command.env("VELNOR_TOOLS_CACHE_KEY", key);
    }
    let run = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(run.status.success(), "{run:?}");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn a_poisoned_restore_names_its_cache_entry_and_the_command_that_deletes_it() {
    let job = job("tools-verify-named-key");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(&job, Some("velnor-tools-abc123"));
    assert!(stdout.contains("::notice::"), "{stdout}");
    assert!(stdout.contains("velnor-tools-abc123"), "{stdout}");
    assert!(
        stdout.contains("gh cache delete 'velnor-tools-abc123' -R tailrocks/velnor-new"),
        "{stdout}"
    );
    assert!(discarded(&job));
}

#[test]
fn a_restore_from_the_node_store_names_no_cache_entry() {
    let job = job("tools-verify-named-none");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(&job, Some(""));
    assert!(stdout.contains("discarded"), "{stdout}");
    assert!(!stdout.contains("gh cache delete"), "{stdout}");
}

#[test]
fn a_hostile_cache_key_cannot_forge_a_second_workflow_command() {
    let job = job("tools-verify-hostile-key");
    recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(&job, Some("velnor-tools-50%\r\n::error::forged"));
    assert_eq!(stdout.lines().count(), 1, "{stdout:?}");
    assert!(
        stdout.contains("gh cache delete 'velnor-tools-50%25%0D%0A::error::forged' -R"),
        "{stdout}"
    );
    assert!(!stdout.contains('\r'), "{stdout:?}");
}
