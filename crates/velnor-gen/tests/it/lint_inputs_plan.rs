// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{EventKind, RepoPath};
use velnor_rust::{detect, lint_runs};

use crate::facts::TestResult;
use crate::lint_inputs::{DECLARED, repo};

fn changed(paths: &[&str]) -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
    paths
        .iter()
        .map(|path| RepoPath::new(*path).map_err(Into::into))
        .collect()
}

fn tracked() -> BTreeSet<RepoPath> {
    [
        "hk.pkl",
        "mise.toml",
        "tools/a/b.sh",
        "rules/a.yml",
        "a/b/x.yml",
        "cfg/check.yml",
    ]
    .iter()
    .filter_map(|path| RepoPath::new(*path).ok())
    .collect()
}

fn runs_of(
    event: EventKind,
    paths: Option<&[&str]>,
) -> Result<Vec<(String, bool)>, Box<dyn std::error::Error>> {
    let facts = detect(&repo(&["Cargo.toml"], DECLARED)?)?;
    let paths = paths.map(changed).transpose()?;
    Ok(lint_runs(
        &facts,
        event,
        paths.as_ref(),
        &tracked(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .into_iter()
    .map(|(slug, run)| (slug.to_string(), run))
    .collect())
}

fn hk(event: EventKind, paths: Option<&[&str]>) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(runs_of(event, paths)?
        .into_iter()
        .find(|(slug, _)| slug == "hk")
        .ok_or("no plan decision for hk")?
        .1)
}

#[test]
fn a_pull_request_that_changes_no_input_skips_the_lint() -> TestResult {
    assert!(!hk(
        EventKind::PullRequest,
        Some(&["src/lib.rs", "README.md"])
    )?);
    assert!(!hk(
        EventKind::PullRequest,
        Some(&["a/hk.pkl", "toolsmith/x"])
    )?);
    Ok(())
}

#[test]
fn a_pull_request_that_changes_an_input_runs_the_lint() -> TestResult {
    for path in ["hk.pkl", "mise.toml", "tools/a/b.sh"] {
        assert!(
            hk(EventKind::PullRequest, Some(&["README.md", path]))?,
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn the_workflow_and_the_config_that_generate_the_lint_always_run_it() -> TestResult {
    for path in [".github/workflows/ci.yml", ".velnor/rust-ci.toml"] {
        assert!(hk(EventKind::PullRequest, Some(&[path]))?, "{path}");
    }
    Ok(())
}

#[test]
fn every_event_but_a_pull_request_with_a_known_change_runs_the_lint() -> TestResult {
    for event in [
        EventKind::Push,
        EventKind::MergeGroup,
        EventKind::Schedule,
        EventKind::WorkflowDispatch,
    ] {
        assert!(hk(event, Some(&["README.md"]))?, "{}", event.name());
        assert!(hk(event, None)?, "{}", event.name());
    }
    assert!(
        hk(EventKind::PullRequest, None)?,
        "unknown change fails open"
    );
    Ok(())
}

#[test]
fn a_lint_without_inputs_has_no_plan_decision() -> TestResult {
    let all = runs_of(EventKind::PullRequest, Some(&["README.md"]))?;
    assert_eq!(all, [("hk".to_owned(), false)]);
    Ok(())
}

#[test]
fn the_tool_manifests_and_the_task_files_of_a_lint_count_as_its_inputs() -> TestResult {
    let bare = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'lint-generator-samples'\ntools = ['jactionlint']\ninputs = ['hk.pkl']\n";
    let facts = detect(&repo(&["Cargo.toml"], bare)?)?;
    let hk = |path: &str| -> Result<bool, Box<dyn std::error::Error>> {
        let paths = changed(&[path])?;
        let runs = lint_runs(
            &facts,
            EventKind::PullRequest,
            Some(&paths),
            &tracked(),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        Ok(*runs.values().next().ok_or("no decision")?)
    };
    for path in [
        "mise.toml",
        "mise.lock",
        ".mise.toml",
        "mise-tasks/check",
        ".config/mise/tasks/lint/hk",
    ] {
        assert!(hk(path)?, "{path}");
    }
    assert!(!hk("src/lib.rs")?);
    let alint = "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\ninputs = ['rules/**']\n";
    let facts = detect(&repo(&["Cargo.toml"], alint)?)?;
    let paths = changed(&["mise-tasks/check"])?;
    let runs = lint_runs(
        &facts,
        EventKind::PullRequest,
        Some(&paths),
        &tracked(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );
    assert_eq!(runs.values().copied().collect::<Vec<_>>(), [false]);
    Ok(())
}

#[test]
fn the_declared_config_and_probe_of_a_lint_count_as_its_inputs() -> TestResult {
    let config = "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\nprobe = 'scripts/probe.sh'\ninputs = ['rules/**']\n";
    let facts = detect(&repo(&["Cargo.toml"], config)?)?;
    let at = |path: &str| -> Result<bool, Box<dyn std::error::Error>> {
        let paths = changed(&[path])?;
        let runs = lint_runs(
            &facts,
            EventKind::PullRequest,
            Some(&paths),
            &tracked(),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        Ok(*runs.values().next().ok_or("no decision")?)
    };
    assert!(at(".alint.yml")?);
    assert!(at("scripts/probe.sh")?);
    assert!(at("rules/a.yml")?);
    assert!(!at("src/lib.rs")?);
    Ok(())
}

#[test]
fn a_tracked_symlink_that_an_input_or_implicit_file_reaches_runs_the_lint() -> TestResult {
    let bare = "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\nprobe = 'scripts/p.sh'\ninputs = ['rules/**', 'a/*/x.yml', 'hk.pkl']\n";
    let facts = detect(&repo(&["Cargo.toml"], bare)?)?;
    let paths = changed(&["real/hk.pkl"])?;
    let decide = |link: &str| -> Result<bool, Box<dyn std::error::Error>> {
        let links = changed(&[link])?;
        let runs = lint_runs(
            &facts,
            EventKind::PullRequest,
            Some(&paths),
            &tracked(),
            &links,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        Ok(*runs.values().next().ok_or("no decision")?)
    };
    for link in [
        "hk.pkl",
        "rules",
        "rules/one.yml",
        "a",
        "a/b",
        "a/b/x.yml",
        "mise.toml",
        ".mise.toml",
        "mise.lock",
        ".alint.yml",
        "scripts",
        "scripts/p.sh",
    ] {
        assert!(decide(link)?, "{link}");
    }
    for link in ["docs/readme.md", "other.pkl", "a/b/c/x.yml"] {
        assert!(!decide(link)?, "{link}");
    }
    Ok(())
}

#[test]
fn a_changed_path_that_is_an_ancestor_of_an_input_runs_the_lint() -> TestResult {
    let config = "[[lint]]\nkind = 'alint'\nconfig = '.alint.yml'\nprobe = 'scripts/p.sh'\ninputs = ['cfg/check.yml']\n";
    let facts = detect(&repo(&["Cargo.toml"], config)?)?;
    for path in ["cfg", "scripts", "cfg/check.yml"] {
        let paths = changed(&[path])?;
        let runs = lint_runs(
            &facts,
            EventKind::PullRequest,
            Some(&paths),
            &tracked(),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        assert_eq!(runs.values().copied().collect::<Vec<_>>(), [true], "{path}");
    }
    let paths = changed(&["cfg2", "docs/cfg"])?;
    let runs = lint_runs(
        &facts,
        EventKind::PullRequest,
        Some(&paths),
        &tracked(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );
    assert_eq!(runs.values().copied().collect::<Vec<_>>(), [false]);
    Ok(())
}

#[test]
fn a_plain_path_input_covers_everything_below_it_and_a_dead_list_runs_the_lint() -> TestResult {
    let config = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'lint-generator-samples'\ninputs = ['cfg']\n";
    let facts = detect(&repo(&["Cargo.toml"], config)?)?;
    let tracked = changed(&["cfg/a/b.pkl", "src/lib.rs"])?;
    let decide =
        |path: &str, tracked: &BTreeSet<RepoPath>| -> Result<bool, Box<dyn std::error::Error>> {
            let paths = changed(&[path])?;
            let runs = lint_runs(
                &facts,
                EventKind::PullRequest,
                Some(&paths),
                tracked,
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeSet::new(),
            );
            Ok(*runs.values().next().ok_or("no decision")?)
        };
    assert!(decide("cfg/a/b.pkl", &tracked)?);
    assert!(!decide("cfgx/a.pkl", &tracked)?);
    assert!(!decide("src/lib.rs", &tracked)?);
    assert!(decide("src/lib.rs", &changed(&["src/lib.rs"])?)?);
    Ok(())
}

#[test]
fn a_file_named_like_the_prefix_of_a_trailing_glob_is_not_a_tracked_input() -> TestResult {
    let config = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'lint-generator-samples'\ninputs = ['cfg/**']\n";
    let facts = detect(&repo(&["Cargo.toml"], config)?)?;
    let paths = changed(&["src/lib.rs"])?;
    let runs = lint_runs(
        &facts,
        EventKind::PullRequest,
        Some(&paths),
        &changed(&["cfg", "src/lib.rs"])?,
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );
    assert_eq!(runs.values().copied().collect::<Vec<_>>(), [true]);
    Ok(())
}

#[test]
fn an_empty_change_list_on_a_pull_request_runs_the_lint() -> TestResult {
    assert!(hk(EventKind::PullRequest, Some(&[]))?);
    Ok(())
}
