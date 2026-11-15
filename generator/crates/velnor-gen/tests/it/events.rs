// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_gen::expected_files;
use velnor_model::{PolicyTools, RepoPath};
use velnor_rust::{CONFIG_PATH, policy_needed};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const EVENTS: &str = r"on:
  pull_request:
    types:
      - opened
      - synchronize
      - reopened
      - ready_for_review
  push:
    branches:
      - main
  merge_group: {}
  workflow_dispatch: {}
permissions:
  contents: read
  pull-requests: read
concurrency:
  group: ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
";

const DENY_MISE: &str = "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"aqua:EmbarkStudios/cargo-deny\" = \"0.18.3\"\n";

const DRAFT: &str = "if: ${{ github.event.pull_request.draft != true }}";

fn with(files: &[(&str, &str)], config: Option<&[u8]>) -> TestResult2 {
    let mut view = view(&["Cargo.toml"], config)?;
    for (path, text) in files {
        let path = RepoPath::new(*path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    workflow(&view)
}

type TestResult2 = Result<String, Box<dyn std::error::Error>>;

fn unquoted(text: &str) -> String {
    text.replace('\'', "")
}

#[test]
fn the_workflow_starts_on_the_four_events_and_cancels_only_older_pull_request_runs() -> TestResult {
    let text = unquoted(&with(&[], None)?);
    assert!(text.contains(&unquoted(EVENTS)), "{text}");
    Ok(())
}

#[test]
fn the_default_branch_comes_from_the_config_and_a_malformed_name_is_refused() -> TestResult {
    let text = with(&[], Some(b"default_branch = \"trunk\"\n"))?;
    assert!(text.contains("    branches:\n      - trunk\n"), "{text}");
    for name in ["", "a..b", "/main", "main/", "ma in", "x\\ny"] {
        let config = format!("default_branch = \"{name}\"\n");
        let mut bad = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        bad.files.entry(CONFIG_PATH).or_default();
        assert!(expected_files(&bad).is_err(), "{name:?}");
    }
    Ok(())
}

#[test]
fn draft_pull_requests_skip_every_lane_but_lint_only_when_the_config_says_so() -> TestResult {
    let shape = "shape = \"lint-test\"\n";
    let off = with(&[], Some(shape.as_bytes()))?;
    assert!(!off.contains("draft"), "{off}");
    let on = with(
        &[],
        Some(format!("{shape}skip_draft_pull_requests = true\n").as_bytes()),
    )?;
    let job = |id: &str| {
        let lines: Vec<&str> = on.lines().skip_while(|line| *line != "jobs:").collect();
        let start = lines.iter().position(|line| *line == format!("  {id}:"));
        lines[start.unwrap_or_default() + 1..]
            .iter()
            .take_while(|line| line.starts_with("   "))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!job("rust-lint").contains("draft"), "{on}");
    assert!(
        unquoted(&job("rust-test")).contains(&unquoted(DRAFT)),
        "{on}"
    );
    assert!(!job("required").contains("draft != true"), "{on}");
    Ok(())
}

#[test]
fn the_policy_lane_exists_only_for_a_deny_config_or_a_pinned_machete() -> TestResult {
    let none = with(&[], None)?;
    assert!(!none.contains("rust-policy"), "{none}");
    let deny = with(&[("deny.toml", ""), ("mise.toml", DENY_MISE)], None)?;
    assert!(deny.contains("  policy:\n"), "{deny}");
    assert!(deny.contains("cargo-deny check"), "{deny}");
    assert!(!deny.contains("cargo-machete"), "{deny}");
    assert!(deny.contains("      - policy\n"), "{deny}");
    let machete = with(
        &[(
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\n",
        )],
        None,
    )?;
    assert!(machete.contains("  policy:\n"), "{machete}");
    assert!(machete.contains("cargo-machete"), "{machete}");
    assert!(!machete.contains("cargo-deny"), "{machete}");
    Ok(())
}

#[test]
fn the_policy_lane_runs_for_a_dependency_file_a_schedule_or_everything_and_not_for_sources()
-> TestResult {
    let changed = |paths: &[&str]| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
        Ok(paths
            .iter()
            .map(|path| RepoPath::new(*path))
            .collect::<Result<_, _>>()?)
    };
    let cases: [(&[&str], bool, bool); 8] = [
        (
            &["src/lib.rs", "crates/a/src/main.rs", "README.md"],
            false,
            false,
        ),
        (&[], false, false),
        (&["Cargo.lock"], false, true),
        (&["crates/a/Cargo.toml"], false, true),
        (&["deny.toml"], false, true),
        (&["docs/deny.toml.md", "src/Cargo.tomls"], false, false),
        (&["src/lib.rs"], true, true),
        (&[], true, true),
    ];
    for (paths, everything, run) in cases {
        assert_eq!(
            policy_needed(&changed(paths)?, everything, PolicyTools::default()),
            run,
            "{paths:?}"
        );
    }
    Ok(())
}

#[test]
fn a_source_change_runs_the_policy_lane_when_machete_is_pinned() -> TestResult {
    let changed: BTreeSet<RepoPath> = BTreeSet::from([RepoPath::new("crates/a/src/lib.rs")?]);
    let deny_only = PolicyTools {
        deny: true,
        machete: false,
    };
    let machete = PolicyTools {
        deny: false,
        machete: true,
    };
    assert!(!policy_needed(&changed, false, deny_only));
    assert!(policy_needed(&changed, false, machete));
    Ok(())
}

#[test]
fn a_deny_config_without_a_pinned_cargo_deny_is_refused() -> TestResult {
    let mut bad = view(&["Cargo.toml"], None)?;
    let deny = RepoPath::new("deny.toml")?;
    bad.tracked_paths.insert(deny.clone());
    bad.files.insert(deny, Vec::new());
    let error = expected_files(&bad).err().map(|error| error.to_string());
    assert!(
        error.is_some_and(|text| text.contains("cargo-deny")),
        "the missing pin must be named"
    );
    Ok(())
}

#[test]
fn a_root_named_policy_cannot_share_a_job_id_with_the_policy_lane() -> TestResult {
    let mut multi = view(&["policy/Cargo.toml", "b/Cargo.toml"], None)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        multi.tracked_paths.insert(path.clone());
        multi.files.insert(path, text.as_bytes().to_vec());
    }
    let text = workflow(&multi)?;
    assert_eq!(text.matches("\n  policy:\n").count(), 1, "{text}");
    assert_eq!(text.matches("\n  rust-policy:\n").count(), 1, "{text}");
    Ok(())
}

#[test]
fn a_draft_run_fails_the_verdict_and_a_conversion_to_draft_starts_a_run() -> TestResult {
    let off = with(&[], None)?;
    assert!(!off.contains("converted_to_draft"), "{off}");
    assert!(!off.contains("draft == true"), "{off}");
    let on = with(&[], Some(b"skip_draft_pull_requests = true\n"))?;
    assert!(on.contains("      - converted_to_draft\n"), "{on}");
    let verdict = on.split("\n  required:\n").nth(1).unwrap_or_default();
    assert!(
        unquoted(verdict).contains("if: ${{ github.event.pull_request.draft == true }}"),
        "{on}"
    );
    Ok(())
}
