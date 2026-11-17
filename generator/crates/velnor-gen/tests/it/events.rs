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
  group: ci-${{ github.workflow_ref }}-${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' && github.event.pull_request.number || github.run_id }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' && startsWith(github.ref, 'refs/pull/') && github.event.pull_request.state != 'closed' }}
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
        let tree = serde_saphyr::from_str::<serde_json::Value>(&text)?;
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

#[test]
fn a_machete_pin_filtered_to_another_os_does_not_enable_the_policy_lane() -> TestResult {
    let mise = |os: &str| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = {{ version = \"0.9.1\", os = [\"{os}\"] }}\n"
        )
    };
    let elsewhere = with(&[("mise.toml", &mise("macos"))], None)?;
    assert!(!elsewhere.contains("cargo-machete"), "{elsewhere}");
    let here = with(&[("mise.toml", &mise("linux"))], None)?;
    assert!(here.contains("cargo-machete"), "{here}");
    let mac = with(
        &[("mise.toml", &mise("macos"))],
        Some(b"runner = \"macos-latest\"\n"),
    )?;
    assert!(mac.contains("cargo-machete"), "{mac}");
    Ok(())
}

#[test]
fn a_machete_pin_reads_mise_os_arch_filters_and_darwin_as_macos() -> TestResult {
    let mise = |os: &str| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = {{ version = \"0.9.1\", os = [\"{os}\"] }}\n"
        )
    };
    let linux_arch = with(
        &[("mise.toml", &mise("linux/x64"))],
        Some(b"runner = \"ubuntu-latest\"\n"),
    )?;
    assert!(linux_arch.contains("cargo-machete"), "{linux_arch}");
    let darwin = with(
        &[("mise.toml", &mise("darwin"))],
        Some(b"runner = \"macos-latest\"\n"),
    )?;
    assert!(darwin.contains("cargo-machete"), "{darwin}");
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
    assert!(unquoted(&job("rust-test")).contains(DRAFT), "{on}");
    assert!(!job("required").contains("draft != true"), "{on}");
    Ok(())
}

#[test]
fn the_policy_lane_exists_only_for_a_deny_config_or_a_pinned_machete() -> TestResult {
    let none = with(&[], None)?;
    assert!(!none.contains("  policy:\n"), "{none}");
    let unpinned = with(&[("deny.toml", "")], None);
    assert!(
        unpinned.is_err_and(|error| error.to_string().contains("cargo-deny is not pinned")),
        "a deny.toml without a pinned cargo-deny must be refused"
    );
    let deny = with(&[("deny.toml", ""), ("mise.toml", DENY_MISE)], None)?;
    assert!(deny.contains("  policy:\n"), "{deny}");
    assert!(
        deny.contains("cargo-deny --config ''deny.toml'' check"),
        "{deny}"
    );
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

type Case<'a> = (&'a [&'a str], bool, bool, (bool, bool));

#[test]
fn the_policy_tools_run_for_their_own_triggers() -> TestResult {
    let changed = |paths: &[&str]| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
        Ok(paths
            .iter()
            .map(|path| RepoPath::new(*path))
            .collect::<Result<_, _>>()?)
    };
    let both = PolicyTools {
        deny: true,
        machete: true,
    };
    let cases: [Case<'_>; 11] = [
        (&["README.md"], false, false, (false, false)),
        (&[], false, false, (false, false)),
        (&["Cargo.lock"], false, true, (true, true)),
        (&["crates/a/Cargo.toml"], false, true, (true, true)),
        (&["deny.toml"], false, true, (true, true)),
        (&[".cargo/config.toml"], false, true, (true, true)),
        (
            &["docs/deny.toml.md", "src/Cargo.tomls"],
            false,
            false,
            (false, false),
        ),
        (&["src/lib.rs", "README.md"], false, true, (false, true)),
        (&["docs/example.rs"], false, false, (false, false)),
        (&["mise.toml"], false, false, (false, true)),
        (&[], true, true, (true, true)),
    ];
    for (paths, everything, selected, (deny, machete)) in cases {
        let runs = policy_runs(both, &changed(paths)?, everything, selected);
        assert_eq!((runs.deny, runs.machete), (deny, machete), "{paths:?}");
    }
    let no_tools = policy_runs(
        PolicyTools::default(),
        &changed(&["Cargo.lock"])?,
        true,
        true,
    );
    assert_eq!(no_tools, PolicyRuns::default());
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
    assert_eq!(
        policy_runs(deny_only, &changed, false, true),
        PolicyRuns::default()
    );
    assert!(policy_runs(machete, &changed, false, true).machete);
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
    assert!(off.contains("      - edited\n"), "{off}");
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
