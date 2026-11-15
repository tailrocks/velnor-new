// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Output;

use crate::io::{Repo, serve};
use crate::io_plan::{plan_command, plan_lock, plan_repo, selected, stale_plan_command};

const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const REPOSITORY: &str = "tailrocks/velnor-lab";

fn event(repo: &Repo, default_branch: &str) -> std::path::PathBuf {
    let file = repo.root().join("event.json");
    let payload = format!(
        "{{\"number\":7,\"repository\":{{\"default_branch\":\"{default_branch}\"}},\"pull_request\":{{\"number\":7,\"base\":{{\"sha\":\"{BASE}\"}}}}}}"
    );
    fs::write(&file, payload).unwrap_or_else(|e| unreachable!("{e}"));
    file
}

fn files(names: &[&str]) -> String {
    let items: Vec<String> = names
        .iter()
        .map(|name| format!("{{\"filename\":\"{name}\",\"status\":\"modified\"}}"))
        .collect();
    format!("[{}]", items.join(","))
}

struct Api(BTreeMap<String, String>);

impl Api {
    fn listing(names: &[&str]) -> Self {
        Self(BTreeMap::from([(
            format!("/repos/{REPOSITORY}/pulls/7/files?per_page=100&page=1"),
            files(names),
        )]))
    }

    fn contents(mut self, path: &str, text: &str) -> Self {
        self.0.insert(
            format!("/repos/{REPOSITORY}/contents/{path}?ref={BASE}"),
            text.to_owned(),
        );
        self
    }
}

fn pull_request(repo: &Repo, api: Option<Api>, token: bool) -> Output {
    let url = serve(api.map(|api| api.0).unwrap_or_default());
    let mut command = plan_command(repo);
    command
        .args(["plan", "--explain"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event(repo, "main"))
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_API_URL", url);
    if token {
        command.env("GITHUB_TOKEN", "t");
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_pull_request_selects_from_the_files_the_api_lists() {
    let repo = plan_repo("api-files");
    let output = pull_request(&repo, Some(Api::listing(&["a/src/lib.rs"])), true);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  a changed:a/src/lib.rs\ntools/Cargo.toml: all:most-packages:1\n  t depends-on:a\n"
    );
}

#[test]
fn a_changed_lockfile_is_compared_with_its_text_at_the_base_commit() {
    let repo = plan_repo("api-lock");
    repo.write("Cargo.lock", &plan_lock("1.0.6"));
    let api = Api::listing(&["Cargo.lock"]).contents("Cargo.lock", &plan_lock("1.0.5"));
    let output = pull_request(&repo, Some(api), true);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  b lock:fnv\ntools/Cargo.toml: none\n"
    );
}

#[test]
fn a_file_list_or_base_text_the_api_does_not_give_selects_everything() {
    let repo = plan_repo("api-down");
    let everything = "Cargo.toml: all:base-unavailable\ntools/Cargo.toml: all:base-unavailable\n";
    let down = pull_request(&repo, None, true);
    assert!(down.status.success(), "{down:?}");
    assert_eq!(selected(&down), everything);
    let anonymous = pull_request(&repo, Some(Api::listing(&["a/src/lib.rs"])), false);
    assert_eq!(selected(&anonymous), everything);
    repo.write("Cargo.lock", &plan_lock("1.0.6"));
    let no_base = pull_request(&repo, Some(Api::listing(&["Cargo.lock"])), true);
    assert_eq!(
        selected(&no_base),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
}

#[test]
fn an_event_other_than_a_pull_request_selects_everything() {
    let repo = plan_repo("api-push");
    for name in ["push", "workflow_dispatch", "merge_group"] {
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
