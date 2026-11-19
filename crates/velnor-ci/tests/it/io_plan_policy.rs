// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, plan_with_bases};

pub(crate) fn policy_lines(repo: &Repo, event: Option<&str>, changed: &[&str]) -> String {
    let outputs = repo.root().join("policy");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command
        .args(["plan", "--changed"])
        .args(changed)
        .env("GITHUB_OUTPUT", &outputs);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| line.starts_with("rust_deny=") || line.starts_with("rust_machete="))
        .flat_map(|line| [line, " "])
        .collect()
}

fn lines_of(repo: &Repo, changed: &[&str], needles: &[&str]) -> String {
    let outputs = repo.root().join("lines");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command
        .args(["plan", "--changed"])
        .args(changed)
        .env("GITHUB_OUTPUT", &outputs);
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| needles.iter().any(|needle| line.contains(needle)))
        .flat_map(|line| [line, " "])
        .collect()
}

#[test]
fn the_doc_flags_of_a_root_come_from_the_selected_packages_only() {
    let repo = plan_repo("doc-flags-selected");
    repo.write(
        "b/src/lib.rs",
        "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n",
    );
    assert_eq!(
        lines_of(
            &repo,
            &["a/src/lib.rs"],
            &["root_mode", "root_doctests", "root_docs"]
        ),
        "rust_root_mode=some rust_root_doctests=skip rust_root_doctests_known=false rust_root_docs=skip "
    );
}

fn pin_machete(repo: &Repo) {
    repo.write(
        "mise.toml",
        "[tools]\nrust = \"1.99.1\"\n\"cargo:cargo-machete\" = \"0.9.1\"\ncargo-deny = \"0.20.2\"\n",
    );
}

#[test]
fn plan_runs_machete_for_a_source_change_and_leaves_deny_skipped() {
    let repo = plan_repo("policy-src");
    pin_machete(&repo);
    repo.write("deny.toml", "");
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["b/src/lib.rs"]),
        "rust_deny=skip rust_machete=run "
    );
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["README.md"]),
        "rust_deny=skip rust_machete=skip "
    );
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["b/.gitignore"]),
        "rust_deny=skip rust_machete=run "
    );
}

#[test]
fn a_source_edit_that_selects_most_of_a_root_is_not_a_global_change_for_cargo_deny() {
    let repo = plan_repo("policy-most");
    pin_machete(&repo);
    repo.write("deny.toml", "");
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["tools/src/main.rs"]),
        "rust_deny=skip rust_machete=run "
    );
}

#[test]
fn plan_runs_machete_when_a_changed_mise_config_pins_it() {
    let repo = plan_repo("policy-mise");
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["mise.toml"]),
        "rust_deny=skip rust_machete=skip "
    );
    pin_machete(&repo);
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["mise.toml"]),
        "rust_deny=skip rust_machete=run "
    );
}

#[test]
fn plan_runs_both_policy_tools_for_a_changed_lock_file_and_on_a_schedule() {
    let repo = plan_repo("policy-lock");
    pin_machete(&repo);
    repo.write("deny.toml", "");
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["Cargo.lock"]),
        "rust_deny=run rust_machete=run "
    );
    assert_eq!(
        policy_lines(&repo, Some("schedule"), &["b/src/lib.rs"]),
        "rust_deny=run rust_machete=run "
    );
}

const DOCTESTS_OFF: &str =
    "[package]\nname = \"a\"\nedition = \"2024\"\n\n[lib]\ndoctest = false\n";
const RUNNER: &str = "[target.x86_64-unknown-linux-gnu]\nrunner = \"true\"\n";

fn warnings_of(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("::warning"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn plan_warns_on_the_pull_request_about_a_setting_the_change_adds_that_makes_a_check_do_less() {
    let repo = plan_repo("weaken");
    repo.write("a/Cargo.toml", DOCTESTS_OFF);
    repo.write(".cargo/config.toml", RUNNER);
    let outputs = repo.root().join("github-output");
    let changed = ["a/Cargo.toml", ".cargo/config.toml"];
    let before = "[package]\nname = \"a\"\nedition = \"2024\"\n";

    let added = plan_with_bases(&repo, &changed, &[("a/Cargo.toml", before)]);
    assert!(added.status.success(), "{added:?}");
    assert_eq!(
        warnings_of(&added),
        [
            "::warning file=.cargo/config.toml::this pull request runs test binaries through \"true\" (target.x86_64-unknown-linux-gnu.runner = \"true\")",
            "::warning file=a/Cargo.toml::this pull request turns off doctests for a ([lib] doctest = false)",
        ]
    );
    let text = String::from_utf8_lossy(&added.stdout);
    assert!(
        text.contains("check does less: a/Cargo.toml: [lib] doctest = false: turns off doctests for a (new in this change)\n"),
        "{text}"
    );

    let kept = plan_with_bases(
        &repo,
        &changed,
        &[
            ("a/Cargo.toml", DOCTESTS_OFF),
            (".cargo/config.toml", RUNNER),
        ],
    );
    assert!(warnings_of(&kept).is_empty(), "{kept:?}");
    let text = String::from_utf8_lossy(&kept.stdout);
    assert!(
        text.contains(
            "check does less: a/Cargo.toml: [lib] doctest = false: turns off doctests for a\n"
        ),
        "{text}"
    );

    let base = repo.root().join("base-a");
    fs::write(&base, before).unwrap_or_else(|e| unreachable!("{e}"));
    let plain = plan_command(&repo)
        .args(["plan", "--changed", "a/Cargo.toml", "--base-file"])
        .arg(format!("a/Cargo.toml={}", base.display()))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&plain.stdout);
    assert!(
        text.starts_with("rust_root_mode=")
            && text.ends_with("turns off doctests for a ([lib] doctest = false)\n"),
        "{text}"
    );
    assert!(!text.contains("check does less"), "{text}");

    let untouched = plan_command(&repo)
        .args(["plan", "--changed", "README.md"])
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(warnings_of(&untouched).is_empty(), "{untouched:?}");
    let written = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!written.contains("::warning"), "{written}");
}

#[test]
fn plan_keeps_deny_skipped_for_a_source_change_in_a_single_crate_repository() {
    let repo = Repo::new("policy-single");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"only\"\nedition = \"2024\"\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    repo.write("src/lib.rs", "");
    repo.write("deny.toml", "");
    pin_machete(&repo);
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["src/lib.rs"]),
        "rust_deny=skip rust_machete=run "
    );
}

#[test]
fn a_changed_mise_config_runs_cargo_deny_when_deny_is_enabled() {
    let repo = plan_repo("policy-mise-deny");
    pin_machete(&repo);
    repo.write("deny.toml", "");
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["mise.toml"]),
        "rust_deny=run rust_machete=run "
    );
}

#[test]
fn plan_runs_cargo_deny_when_the_change_deletes_its_policy() {
    let repo = plan_repo("policy-deleted");
    pin_machete(&repo);
    assert_eq!(
        policy_lines(&repo, Some("pull_request"), &["deny.toml"]),
        "rust_deny=run rust_machete=run "
    );
}
