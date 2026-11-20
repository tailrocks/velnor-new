// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, selected, stale_plan_command};

pub(crate) fn event(repo: &Repo, default_branch: &str) -> std::path::PathBuf {
    let file = repo.root().join("event.json");
    let payload = format!(
        "{{\"number\":7,\"repository\":{{\"default_branch\":\"{default_branch}\"}},\"pull_request\":{{\"number\":7,\"base\":{{\"sha\":\"0123456789abcdef0123456789abcdef01234567\"}}}}}}"
    );
    fs::write(&file, payload).unwrap_or_else(|e| unreachable!("{e}"));
    file
}

#[test]
fn an_event_that_is_neither_a_pull_request_nor_a_push_to_the_default_branch_selects_everything() {
    let repo = plan_repo("api-push");
    for name in ["schedule", "workflow_dispatch"] {
        let output = plan_command(&repo)
            .args(["plan", "--explain"])
            .env("GITHUB_EVENT_NAME", name)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{name}: {output:?}");
        assert_eq!(
            selected(&output),
            "Cargo.toml: all:event\ntools/Cargo.toml: all:event\n",
            "{name}"
        );
    }
}

#[test]
fn a_default_branch_that_differs_from_the_repositorys_fails_the_plan() {
    let repo = plan_repo("api-branch");
    repo.write(".velnor/rust-ci.toml", "default_branch = \"trunk\"\n");
    let run = |actual: &str| {
        plan_command(&repo)
            .arg("plan")
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_EVENT_PATH", event(&repo, actual))
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    assert!(run("trunk").status.success());
    let wrong = run("main");
    assert_eq!(wrong.status.code(), Some(1));
    let message = String::from_utf8_lossy(&wrong.stderr);
    assert!(
        message.contains("\"trunk\"") && message.contains("\"main\""),
        "{message}"
    );
}

#[test]
fn a_cargo_toml_edited_after_generate_fails_the_plan_naming_the_command_to_run() {
    let repo = plan_repo("api-stale");
    let fresh = plan_command(&repo)
        .args(["plan", "--changed", "a/src/lib.rs"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(fresh.status.success(), "{fresh:?}");
    repo.write(
        "a/Cargo.toml",
        "[package]\nname = \"a\"\nedition = \"2024\"\n\n[dependencies]\n",
    );
    let stale = stale_plan_command(&repo)
        .args(["plan", "--changed", "a/src/lib.rs"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(stale.status.code(), Some(1));
    let message = String::from_utf8_lossy(&stale.stderr);
    assert!(
        message.contains("a/Cargo.toml") && message.ends_with("run velnor-gen generate\n"),
        "{message}"
    );
    repo.write(
        "c/Cargo.toml",
        "[package]\nname = \"c\"\nedition = \"2024\"\n",
    );
    repo.write("c/src/lib.rs", "");
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n",
    );
    let moved = stale_plan_command(&repo)
        .args(["plan", "--changed", "a/src/lib.rs"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(moved.status.code(), Some(1));
    fs::remove_file(repo.root().join(".velnor/rust-metadata.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let missing = stale_plan_command(&repo)
        .args(["plan", "--changed", "a/src/lib.rs"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains(".velnor/rust-metadata.json is missing; run velnor-gen generate")
    );
}

#[test]
fn the_plan_starts_no_cargo_rustup_or_mise() {
    let repo = plan_repo("api-no-toolchain");
    let fake = repo.root().join("../velnor-ci-plan-fake-bin");
    fs::create_dir_all(&fake).unwrap_or_else(|e| unreachable!("{e}"));
    let marker = fake.join("started");
    for program in ["cargo", "rustc", "rustup", "mise", "cargo-nextest"] {
        let script = fake.join(program);
        fs::write(
            &script,
            format!(
                "#!/bin/sh\necho {program} >> '{}'\nexit 99\n",
                marker.display()
            ),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut command = plan_command(&repo);
    let output = command
        .args(["plan", "--changed", "a/src/lib.rs"])
        .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let started = fs::read_to_string(&marker).unwrap_or_default();
    fs::remove_dir_all(&fake).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(started, "");
}
