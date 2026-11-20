// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_model::{DenyCheck, DenyLane, PolicyTable, PolicyTools, RepoPath, RepoView};
use velnor_rust::{DENY_OFF_NOTE, detect, policy_runs};

use super::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const DENY_RUN: &str = "cargo-deny --config ''deny.toml'' --locked check";

fn repo(config: &[u8], deny_toml: bool, helper: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config))?;
    if deny_toml {
        let path = RepoPath::new("deny.toml")?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, Vec::new());
        repo.files
            .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    }
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn notes(repo: &RepoView) -> Result<Vec<String>, Box<dyn Error>> {
    let mut locked = repo.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    Ok(velnor_gen::expected_with_notes(&locked)?.1)
}

#[test]
fn an_unset_policy_table_runs_the_lane_when_a_deny_toml_exists() -> TestResult {
    let text = workflow(&repo(b"", true, "0.1.110")?)?;
    assert!(text.contains(DENY_RUN), "{text}");
    Ok(())
}

#[test]
fn deny_false_turns_the_lane_off_even_with_a_deny_toml_and_says_so_in_the_report() -> TestResult {
    let repo = repo(b"[policy]\ndeny = false\n", true, "0.1.110")?;
    let text = workflow(&repo)?;
    assert!(!text.contains("cargo-deny --"), "{text}");
    assert!(!text.contains("fetch db"), "{text}");
    assert!(notes(&repo)?.iter().any(|note| note == DENY_OFF_NOTE));
    Ok(())
}

#[test]
fn deny_false_without_a_deny_toml_says_nothing_because_nothing_is_ignored() -> TestResult {
    let repo = repo(b"[policy]\ndeny = false\n", false, "0.1.110")?;
    assert!(!workflow(&repo)?.contains("cargo-deny --"));
    assert!(!notes(&repo)?.iter().any(|note| note == DENY_OFF_NOTE));
    Ok(())
}

#[test]
fn deny_true_runs_the_lane_with_a_deny_toml_and_is_an_error_without_one() -> TestResult {
    let text = workflow(&repo(b"[policy]\ndeny = true\n", true, "0.1.110")?)?;
    assert!(text.contains(DENY_RUN), "{text}");
    let error = workflow(&repo(b"[policy]\ndeny = true\n", false, "0.1.110")?)
        .err()
        .ok_or("deny = true without a deny.toml was accepted")?;
    assert!(error.to_string().contains("deny = true"), "{error}");
    assert!(error.to_string().contains("deny.toml"), "{error}");
    Ok(())
}

#[test]
fn the_policy_table_is_refused_by_a_helper_older_than_0_1_110() -> TestResult {
    for config in [&b"[policy]\ndeny = false\n"[..], b"[policy]\n"] {
        let error = workflow(&repo(config, true, "0.1.107")?)
            .err()
            .ok_or("an older helper accepted [policy]")?;
        assert!(error.to_string().contains("0.1.110"), "{error}");
    }
    Ok(())
}

#[test]
fn deny_checks_run_only_the_listed_checks_in_their_fixed_order() -> TestResult {
    let text = workflow(&repo(
        b"[policy]\ndeny_checks = [\"sources\", \"bans\", \"licenses\"]\n",
        true,
        "0.1.110",
    )?)?;
    assert!(
        text.contains("cargo-deny --config ''deny.toml'' --locked check bans licenses sources"),
        "{text}"
    );
    Ok(())
}

#[test]
fn deny_checks_name_known_checks_once_and_at_least_one() -> TestResult {
    for checks in ["[]", "[\"fmt\"]", "[\"bans\", \"bans\"]", "\"bans\""] {
        let config = format!("[policy]\ndeny_checks = {checks}\n");
        assert!(
            workflow(&repo(config.as_bytes(), true, "0.1.110")?).is_err(),
            "{checks} accepted"
        );
    }
    Ok(())
}

#[test]
fn the_parser_keeps_the_written_policy_table() -> TestResult {
    let config = detect(&repo(
        b"[policy]\ndeny = false\ndeny_checks = [\"bans\"]\n",
        true,
        "0.1.110",
    )?)?
    .config;
    assert_eq!(
        config.policy,
        Some(PolicyTable {
            deny: DenyLane::Off,
            deny_checks: Some(vec![DenyCheck::Bans]),
        })
    );
    assert_eq!(detect(&repo(b"", true, "0.1.110")?)?.config.policy, None);
    Ok(())
}

#[test]
fn a_changed_deny_toml_selects_no_policy_run_while_the_lane_is_off() -> TestResult {
    let off = PolicyTools {
        deny: false,
        machete: false,
        deny_off: true,
    };
    let changed = BTreeSet::from([RepoPath::new("deny.toml")?]);
    assert!(!policy_runs(off, &changed, false, true).deny);
    Ok(())
}
