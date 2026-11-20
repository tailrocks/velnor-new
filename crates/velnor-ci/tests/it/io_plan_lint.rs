// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;

use velnor_model::{PathGlob, RepoPath};
use velnor_rust::unmatched_entries;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo};

const CONFIG: &str = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'hk'\ninputs = ['hk.pkl', 'mise.toml']\n\n[[lint]]\nkind = 'mise-task'\nid = 'plain'\ntask = 'hk'\n";

fn lint_repo(name: &str) -> Repo {
    let repo = plan_repo(name);
    repo.write_all(&[
        (".velnor/rust-ci.toml", CONFIG),
        (
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\n[tasks.hk]\nrun = \"true\"\n",
        ),
    ]);
    repo
}

fn lint_lines(repo: &Repo, event: Option<&str>, changed: &[&str]) -> String {
    let outputs = repo.root().join("lint-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command.arg("plan");
    if !changed.is_empty() {
        command.arg("--changed").args(changed);
    }
    command.env("GITHUB_OUTPUT", &outputs);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| line.starts_with("lint_"))
        .flat_map(|line| [line, " "])
        .collect()
}

#[test]
fn plan_skips_a_lint_on_a_pull_request_that_changes_none_of_its_inputs() {
    let repo = lint_repo("lint-skip");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs"]),
        "lint_hk=skip "
    );
}

#[test]
fn plan_runs_a_lint_on_a_pull_request_that_changes_one_of_its_inputs() {
    let repo = lint_repo("lint-run");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs", "hk.pkl"]),
        "lint_hk=run "
    );
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["mise.toml"]),
        "lint_hk=run "
    );
}

#[test]
fn plan_runs_a_lint_on_push_merge_group_schedule_and_dispatch() {
    let repo = lint_repo("lint-events");
    for event in ["push", "merge_group", "schedule", "workflow_dispatch"] {
        assert_eq!(
            lint_lines(&repo, Some(event), &["a/src/lib.rs"]),
            "lint_hk=run ",
            "{event}"
        );
    }
}

#[test]
fn plan_runs_a_lint_when_it_cannot_list_the_changed_files() {
    let repo = lint_repo("lint-open");
    assert_eq!(lint_lines(&repo, Some("pull_request"), &[]), "lint_hk=run ");
}

#[test]
fn plan_writes_no_lint_output_for_a_repository_without_lint_inputs() {
    let repo = plan_repo("lint-none");
    assert_eq!(lint_lines(&repo, Some("pull_request"), &["README.md"]), "");
}

#[test]
fn plan_runs_a_lint_whose_input_is_a_symlink_when_only_the_target_changed() {
    let repo = lint_repo("lint-symlink");
    repo.write("real/hk.pkl", "amends \"x\"\n");
    symlink("real/hk.pkl", repo.root().join("hk.pkl")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["real/hk.pkl"]),
        "lint_hk=run "
    );
}

#[test]
fn plan_runs_a_lint_whose_input_sits_below_a_symlinked_folder() {
    let repo = lint_repo("lint-symlink-folder");
    repo.write(
        ".velnor/rust-ci.toml",
        "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'hk'\ninputs = ['cfg/hk.pkl']\n",
    );
    repo.write("real/hk.pkl", "x\n");
    symlink("real", repo.root().join("cfg")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["real/hk.pkl"]),
        "lint_hk=run "
    );
}

#[test]
fn plan_still_skips_a_lint_when_no_symlink_reaches_its_inputs() {
    let repo = lint_repo("lint-symlink-unrelated");
    repo.write("real/other.txt", "x\n");
    symlink("real/other.txt", repo.root().join("other.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["real/other.txt"]),
        "lint_hk=skip "
    );
}

#[test]
fn plan_reports_a_skipped_lint_and_why_in_the_summary_and_the_explanation() {
    let repo = lint_repo("lint-report");
    let summary = repo.root().join("summary");
    fs::write(&summary, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", "a/src/lib.rs"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let line = "lint-hk | skip | no changed file reaches its inputs";
    let text = fs::read_to_string(&summary).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(text.contains(line), "{text}");
    let explained = String::from_utf8_lossy(&output.stdout);
    assert!(
        explained.contains("left out: lint-hk: no changed file reaches its inputs"),
        "{explained}"
    );
}

#[test]
fn plan_reports_no_lint_line_for_a_lint_that_runs() {
    let repo = lint_repo("lint-report-run");
    let summary = repo.root().join("summary");
    fs::write(&summary, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", "hk.pkl"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let text = fs::read_to_string(&summary).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!text.contains("lint-hk"), "{text}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("left out: lint-hk"));
}

fn folder_repo(name: &str, inputs: &str) -> Repo {
    let repo = lint_repo(name);
    repo.write_all(&[
        (
            ".velnor/rust-ci.toml",
            &format!("[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'hk'\ninputs = {inputs}\n"),
        ),
        ("cfg/hk.pkl", "x\n"),
        ("cfg/sub/deep.pkl", "x\n"),
        ("cfgx/other.pkl", "x\n"),
    ]);
    repo
}

#[test]
fn plan_runs_a_lint_whose_input_is_a_folder_when_a_file_below_it_changed() {
    let repo = folder_repo("lint-folder-run", "['cfg']");
    for changed in ["cfg/hk.pkl", "cfg/sub/deep.pkl"] {
        assert_eq!(
            lint_lines(&repo, Some("pull_request"), &[changed]),
            "lint_hk=run ",
            "{changed}"
        );
    }
}

#[test]
fn plan_skips_a_lint_whose_input_is_a_folder_when_only_a_sibling_prefix_changed() {
    let repo = folder_repo("lint-folder-skip", "['cfg']");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["cfgx/other.pkl"]),
        "lint_hk=skip "
    );
}

#[test]
fn plan_runs_a_lint_none_of_whose_inputs_matches_a_tracked_file() {
    let repo = folder_repo("lint-dead-inputs", "['cfgg', 'nothing/**', 'a/*/x.yml']");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs"]),
        "lint_hk=run "
    );
}

#[test]
fn plan_still_skips_a_lint_when_only_an_optional_input_is_untracked() {
    let repo = folder_repo("lint-optional-input", "['cfg/hk.pkl', 'cfg/hk.local.pkl']");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs"]),
        "lint_hk=skip "
    );
}

#[test]
fn a_lint_input_with_a_question_mark_or_a_bracket_is_refused_at_config_load() {
    for inputs in [
        "['cfg/h?.pkl', 'mise.toml']",
        "['cfg/h[k].pkl', 'mise.toml']",
    ] {
        let repo = folder_repo("lint-glob-refused", inputs);
        let text = velnor_gen::io::write_metadata(repo.root())
            .err()
            .unwrap_or_else(|| unreachable!("the config with {inputs} was accepted"))
            .to_string();
        assert!(
            text.contains("only * and ** are supported"),
            "{inputs}: {text}"
        );
    }
}

#[test]
fn plan_explain_warns_about_an_input_that_matches_no_tracked_file_and_keeps_the_decision() {
    let repo = folder_repo("lint-typo-warning", "['cfg/hk.pkl', 'cfgg/hk.pkl']");
    let summary = repo.root().join("summary");
    fs::write(&summary, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", "a/src/lib.rs"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let explained = String::from_utf8_lossy(&output.stdout);
    assert!(
        explained
            .contains("warning: lint-hk: inputs entry \"cfgg/hk.pkl\" matches no tracked file\n"),
        "{explained}"
    );
    assert!(!explained.contains("cfg/hk.pkl\" matches"), "{explained}");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs"]),
        "lint_hk=skip "
    );
}

#[test]
fn plan_explain_does_not_warn_about_an_input_of_a_lint_that_runs_on_a_changed_input() {
    let repo = folder_repo("lint-typo-runs", "['cfg/hk.pkl', 'cfgg/hk.pkl']");
    let summary = repo.root().join("summary");
    fs::write(&summary, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", "cfg/hk.pkl"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let explained = String::from_utf8_lossy(&output.stdout);
    assert!(!explained.contains("matches no tracked file"), "{explained}");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["cfg/hk.pkl"]),
        "lint_hk=run "
    );
}

#[test]
fn unmatched_inputs_take_one_pass_over_the_tracked_paths() {
    let tracked: BTreeSet<RepoPath> = (0..20_000)
        .map(|i| {
            RepoPath::new(format!("lint{}/src/{i}.rs", i % 120))
                .unwrap_or_else(|e| unreachable!("{e}"))
        })
        .collect();
    let entries: Vec<(usize, PathGlob)> = (0..120)
        .map(|k| {
            let folder = if k % 2 == 0 {
                format!("lint{k}")
            } else {
                format!("typo{k}")
            };
            (k, PathGlob::new(folder).unwrap_or_else(|e| unreachable!("{e}")))
        })
        .collect();
    let calls = Cell::new(0_usize);
    let unmatched = unmatched_entries(entries, &tracked, |glob, path| {
        calls.set(calls.get() + 1);
        path.as_str()
            .strip_prefix(glob.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    });
    let names: Vec<usize> = unmatched.iter().map(|(k, _)| *k).collect();
    assert_eq!(names, (0..120).filter(|k| k % 2 == 1).collect::<Vec<_>>());
    assert!(
        calls.get() <= tracked.len(),
        "{} matcher calls for {} tracked paths",
        calls.get(),
        tracked.len()
    );
}
