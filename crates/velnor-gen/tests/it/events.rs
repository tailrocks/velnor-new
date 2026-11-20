// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_gen::expected_files;
use velnor_model::{PolicyTools, RepoPath};
use velnor_rust::{CONFIG_PATH, PolicyRuns, policy_runs};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const EVENTS: &str = r"on:
  pull_request:
    types:
      - opened
      - synchronize
      - reopened
      - ready_for_review
      - edited
  push:
    branches:
      - main
  merge_group: {}
  workflow_dispatch: {}
permissions:
  contents: read
concurrency:
  group: ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && !(github.event.action == 'edited' && github.event.changes.base == null) && github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && !(github.event.action == 'edited' && github.event.changes.base == null) }}
";

pub(crate) const DENY_MISE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:EmbarkStudios/cargo-deny\" = \"0.18.3\"\n";

const DRAFT: &str = "github.event.pull_request.draft != true";
const ACTIVE_EVENT: &str = "(github.event_name != 'pull_request' || (startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed'))";

#[test]
fn every_ci_job_excludes_closed_pull_requests_and_preserves_other_events() -> TestResult {
    let workflows = [
        with(&[], None)?,
        with(&[], Some(b"shape = \"lint-test\"\n"))?,
        workflow(&view(&["Cargo.toml", "tools/Cargo.toml"], None)?)?,
        with(
            &[("deny.toml", ""), ("mise.toml", DENY_MISE)],
            Some(b"shape = \"lint-test\"\nskip_draft_pull_requests = true\n"),
        )?,
        workflow(&crate::golden::load(
            &crate::golden::samples().join("helper-source"),
        )?)?,
    ];
    for text in workflows {
        let tree = crate::yaml_tree::document(&text)?;
        assert!(
            tree.pointer("/on/pull_request/types")
                .ok_or("no pull request trigger")?
                .as_array()
                .ok_or("no event types")?
                .iter()
                .any(|kind| kind.as_str() == Some("edited")),
            "{text}"
        );
        let jobs = tree
            .get("jobs")
            .and_then(serde_json::Value::as_object)
            .ok_or("no jobs")?;
        for id in jobs.keys() {
            let condition = jobs
                .get(id)
                .and_then(|job| job.get("if"))
                .and_then(serde_json::Value::as_str);
            assert!(
                condition.is_some_and(|value| {
                    value == format!("${{{{ {ACTIVE_EVENT} }}}}")
                        || value.starts_with(&format!("${{{{ {ACTIVE_EVENT} && ("))
                            && value.ends_with(") }}")
                }),
                "{id} must reject closed PR edits and merged PRs with empty payloads while admitting pull refs with open or unknown PR state and non-PR events: {condition:?}"
            );
        }
        assert_eq!(
            jobs.get("required")
                .and_then(|job| job.get("if"))
                .and_then(serde_json::Value::as_str),
            Some(format!("${{{{ {ACTIVE_EVENT} && (always()) }}}}").as_str())
        );
        if let Some(condition) = jobs
            .get("rust-test")
            .and_then(|job| job.get("if"))
            .and_then(serde_json::Value::as_str)
        {
            assert!(
                condition.contains("needs.rust-plan.outputs.rust_root_mode != 'none'"),
                "{condition}"
            );
            if jobs.get("policy").is_some() {
                assert!(condition.contains(DRAFT), "{condition}");
                let policy = jobs
                    .get("policy")
                    .and_then(|job| job.get("if"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or("no policy condition")?;
                assert!(policy.contains("needs.rust-plan.outputs.rust_deny == 'run' || needs.rust-plan.outputs.rust_machete == 'run'"), "{policy}");
            }
        }
    }
    Ok(())
}

fn with(files: &[(&str, &str)], config: Option<&[u8]>) -> TestResult2 {
    let mut view = view(&["Cargo.toml"], config)?;
    for (path, text) in files {
        let path = RepoPath::new(*path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    crate::facts::lock_pinned_tools(&mut view)?;
    workflow(&view)
}

type TestResult2 = Result<String, Box<dyn std::error::Error>>;

fn unquoted(text: &str) -> String {
    text.replace('\'', "")
}

#[test]
fn the_workflow_starts_on_the_four_events_and_cancels_only_older_pull_request_runs() -> TestResult {
    let text = unquoted(&with(&[], Some(b"full_run = \"off\"\n"))?);
    assert!(text.contains(&unquoted(EVENTS)), "{text}");
    Ok(())
}

#[test]
fn the_default_branch_comes_from_the_config_and_a_malformed_name_is_refused() -> TestResult {
    let text = with(&[], Some(b"default_branch = \"trunk\"\n"))?;
    assert!(text.contains("    branches:\n      - trunk\n"), "{text}");
    for name in [
        "",
        "a..b",
        "/main",
        "main/",
        "ma in",
        "x\\ny",
        ".release",
        "feature/.hidden",
        "release.lock",
        "feature/x.lock/y",
        "${{secrets.T}}",
        "a{{b",
        "a}}b",
    ] {
        let config = format!("default_branch = \"{name}\"\n");
        let mut bad = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        bad.files.entry(CONFIG_PATH).or_default();
        assert!(expected_files(&bad).is_err(), "{name:?}");
    }
    Ok(())
}

#[test]
fn a_default_branch_with_a_filter_character_is_escaped_so_the_push_filter_matches_it_literally()
-> TestResult {
    for (name, filter) in [
        ("release/c++", "release/c\\+\\+"),
        ("!hotfix", "\\!hotfix"),
        ("v1]", "v1\\]"),
    ] {
        let config = format!("default_branch = \"{name}\"\n");
        let text = with(&[], Some(config.as_bytes()))?;
        assert!(
            text.contains(&format!("    branches:\n      - '{filter}'\n")),
            "{name}: {text}"
        );
    }
    Ok(())
}

mod policy;

