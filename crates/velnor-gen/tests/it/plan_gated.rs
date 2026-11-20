// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_gen::{GenerateError, expected_files};
use velnor_model::{EventKind, RepoPath, RepoView};
use velnor_rust::{LOCK_PATH, detect, lint_runs};

use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::lint_lane::{LINT, repo as lint_repo};
use crate::lock_archive::archive_lock_at;
use crate::pipeline::{pin_helper, view, workflow};

const POLICY_GATED: &[u8] = b"[policy]\ngate_on_plan = true\n";
const CHECK_GATED: &str = "[[lint]]\nkind = 'generated-check'\ngate_on_plan = true\n";
const CHECK_PLAIN: &str = "[[lint]]\nkind = 'generated-check'\n";
const FIRST: &str = "0.1.111";

fn policy_repo(
    config: Option<&[u8]>,
    helper: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], config)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, text.as_bytes().to_vec());
    }
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn check_repo(config: &str, helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = lint_repo(&format!("{LINT}\n{config}"))?;
    crate::facts::lock_pinned_tools(&mut repo)?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files
        .insert(LOCK_PATH, archive_lock_at(helper)?.into_bytes());
    Ok(repo)
}

fn job(text: &str, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    crate::yaml_tree::document(text)?
        .pointer(&format!("/jobs/{id}"))
        .cloned()
        .ok_or_else(|| format!("no job {id}").into())
}

fn condition(job: &Value) -> String {
    job["if"].as_str().unwrap_or_default().to_owned()
}

fn plans(job: &Value) -> bool {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|step| step["id"].as_str() == Some("plan"))
}

fn selected(paths: &[&str], event: EventKind) -> Result<bool, Box<dyn std::error::Error>> {
    let facts = detect(&check_repo(CHECK_GATED, FIRST)?)?;
    let changed: BTreeSet<RepoPath> = paths
        .iter()
        .map(|path| RepoPath::new(*path))
        .collect::<Result<_, _>>()?;
    let runs = lint_runs(&facts, event, Some(&changed), &BTreeSet::new());
    runs.into_iter()
        .find(|(slug, _)| slug.as_str() == "generated_check")
        .map(|(_, run)| run)
        .ok_or_else(|| "no plan decision for the generated check".into())
}

#[test]
fn a_gated_policy_job_waits_for_the_plan_job_and_is_skipped_by_its_job_condition() -> TestResult {
    let text = workflow(&policy_repo(Some(POLICY_GATED), FIRST)?)?;
    let policy = job(&text, "policy")?;
    assert_eq!(
        policy["needs"],
        serde_json::json!(["rust-plan"]),
        "{policy}"
    );
    assert!(!plans(&policy), "{policy}");
    let condition = condition(&policy);
    assert!(
        condition.contains("needs.rust-plan.outputs.rust_deny == 'run'"),
        "{condition}"
    );
    assert!(job(&text, "rust-plan").is_ok(), "{text}");
    let rust = job(&text, "rust")?;
    assert_eq!(rust["needs"], serde_json::json!(["rust-plan"]), "{rust}");
    Ok(())
}

#[test]
fn a_policy_job_without_the_key_still_plans_alone_and_allocates_a_runner() -> TestResult {
    let text = workflow(&policy_repo(None, FIRST)?)?;
    let policy = job(&text, "policy")?;
    assert!(policy["needs"].is_null(), "{policy}");
    assert!(plans(&policy), "{policy}");
    assert!(job(&text, "rust-plan").is_err());
    Ok(())
}

#[test]
fn a_helper_older_than_the_policy_key_is_refused() -> TestResult {
    let repo = policy_repo(Some(POLICY_GATED), "0.1.110")?;
    let error = expected_files(&repo).err();
    assert!(
        matches!(
            error,
            Some(GenerateError::ConfigKeyHelperTooOld {
                first: (0, 1, 111),
                ..
            })
        ),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_gated_generated_check_waits_for_the_plan_and_runs_only_when_the_plan_says_run() -> TestResult {
    let text = workflow(&check_repo(CHECK_GATED, FIRST)?)?;
    let check = job(&text, "lint-generated-check")?;
    assert_eq!(check["needs"], serde_json::json!(["rust-plan"]), "{check}");
    let condition = condition(&check);
    assert!(
        condition.contains("needs.rust-plan.outputs.lint_generated_check == 'run'"),
        "{condition}"
    );
    let plan = job(&text, "rust-plan")?;
    assert!(
        plan["outputs"]["lint_generated_check"].is_string(),
        "{plan}"
    );
    Ok(())
}

#[test]
fn an_ungated_generated_check_still_runs_on_every_pull_request_with_no_plan() -> TestResult {
    let text = workflow(&check_repo(CHECK_PLAIN, FIRST)?)?;
    let check = job(&text, "lint-generated-check")?;
    assert!(check["needs"].is_null(), "{check}");
    assert!(!condition(&check).contains("lint_"), "{check}");
    assert!(job(&text, "rust-plan").is_err());
    Ok(())
}

#[test]
fn a_docs_only_pull_request_skips_the_generated_check() -> TestResult {
    for paths in [
        &["README.md"][..],
        &["docs/readme.md", "docs/guide/start.md"],
        &["crates/velnor-gen/README.md"],
    ] {
        assert!(!selected(paths, EventKind::PullRequest)?, "{paths:?}");
    }
    Ok(())
}

#[test]
fn a_pull_request_that_touches_a_generator_input_runs_the_generated_check() -> TestResult {
    for path in [
        ".github/workflows/rust-ci.yml",
        ".github/workflows/release.yml",
        ".github/PULL_REQUEST_TEMPLATE.md",
        ".velnor/rust-ci.toml",
        ".velnor/release.toml",
        ".velnor/rust-ci.lock",
        "generator/CONFIG.md",
        "Cargo.toml",
        "Cargo.lock",
        "crates/a/Cargo.toml",
        "crates/a/src/lib.rs",
        "mise.toml",
        "mise.lock",
        "rust-toolchain.toml",
        "deny.toml",
        "docs/diagram.svg",
        "docs/script.sh",
    ] {
        assert!(
            selected(&["docs/readme.md", path], EventKind::PullRequest)?,
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn every_other_event_runs_the_gated_generated_check_even_for_a_docs_change() -> TestResult {
    for event in [
        EventKind::Push,
        EventKind::MergeGroup,
        EventKind::Schedule,
        EventKind::WorkflowDispatch,
    ] {
        assert!(selected(&["README.md"], event)?, "{}", event.name());
    }
    Ok(())
}

#[test]
fn an_empty_change_list_on_a_pull_request_runs_the_gated_generated_check() -> TestResult {
    assert!(
        selected(&[], EventKind::PullRequest)?,
        "an empty change list is unknown, not docs-only"
    );
    Ok(())
}

#[test]
fn an_unlisted_change_set_runs_the_gated_generated_check() -> TestResult {
    let facts = detect(&check_repo(CHECK_GATED, FIRST)?)?;
    let runs = lint_runs(&facts, EventKind::PullRequest, None, &BTreeSet::new());
    assert!(runs.values().all(|run| *run), "{runs:?}");
    Ok(())
}

#[test]
fn a_tracked_symlink_runs_the_gated_generated_check_unless_it_is_a_markdown_file() -> TestResult {
    let facts = detect(&check_repo(CHECK_GATED, FIRST)?)?;
    let changed: BTreeSet<RepoPath> = [RepoPath::new("README.md")?].into();
    for (link, expected) in [("CLAUDE.md", false), ("Cargo.toml", true)] {
        let links: BTreeSet<RepoPath> = [RepoPath::new(link)?].into();
        let runs = lint_runs(&facts, EventKind::PullRequest, Some(&changed), &links);
        assert_eq!(
            runs.get(&velnor_model::RootSlug::new("generated_check")?),
            Some(&expected),
            "{link}"
        );
    }
    Ok(())
}

#[test]
fn the_key_is_refused_on_any_other_lint_and_by_a_helper_older_than_it() -> TestResult {
    let foreign = lint_repo("[[lint]]\nkind = 'jactionlint'\ngate_on_plan = true\n")?;
    let text = format!("{:?}", detect(&foreign).err());
    assert!(text.contains("gate_on_plan"), "{text}");
    let error = expected_files(&check_repo(CHECK_GATED, "0.1.110")?).err();
    assert!(
        matches!(
            error,
            Some(GenerateError::ConfigKeyHelperTooOld {
                first: (0, 1, 111),
                ..
            })
        ),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn the_required_gate_expects_a_gated_generated_check_only_when_the_plan_says_run() -> TestResult {
    for (config, conditional) in [(CHECK_GATED, true), (CHECK_PLAIN, false)] {
        let text = workflow(&check_repo(config, FIRST)?)?;
        let spec = crate::lint_gate::spec(&text)?;
        let check = spec
            .jobs
            .iter()
            .find(|job| job.id.as_str() == "lint-generated-check")
            .ok_or("the gate spec lacks the generated check")?;
        assert_eq!(!check.when.is_empty(), conditional, "{check:?}");
    }
    Ok(())
}
