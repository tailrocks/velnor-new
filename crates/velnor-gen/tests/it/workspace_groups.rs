// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const GROUP: &str = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";
const LOCK: &str = ".velnor/rust-ci.lock";
const ROOTS: [&str; 3] = ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];

fn grouped(config: &str) -> Result<String, Box<dyn Error>> {
    let mut view = view(&ROOTS, Some(config.as_bytes()))?;
    crate::pipeline::pin_helper(&mut view, "0.1.96")?;
    workflow(&view)
}

fn job<'t>(text: &'t str, id: &str) -> &'t str {
    let marker = format!("\n  {id}:\n");
    let Some(start) = text.find(&marker) else {
        return "";
    };
    let rest = &text[start + 1..];
    let mut end = rest.len();
    let mut from = 0;
    while let Some(found) = rest[from..].find("\n  ") {
        let at = from + found;
        if !rest[at + 3..].starts_with(' ') {
            end = at;
            break;
        }
        from = at + 1;
    }
    &rest[..end]
}

fn refused(config: &str) -> Result<String, Box<dyn Error>> {
    match view(&ROOTS, Some(config.as_bytes())) {
        Ok(view) => match workflow(&view) {
            Ok(text) => Err(format!("the workflow was generated:\n{text}").into()),
            Err(error) => Ok(error.to_string()),
        },
        Err(error) => Err(error),
    }
}

#[test]
fn a_group_of_two_roots_gets_one_job_set_that_names_each_manifest() -> TestResult {
    let text = grouped(GROUP)?;
    assert!(text.contains("\n  rust-tools:\n"), "{text}");
    assert!(!text.contains("\n  rust-tools_a:\n"), "{text}");
    assert!(!text.contains("\n  rust-tools_b:\n"), "{text}");
    assert!(
        text.contains("--manifest-path ''tools/a/Cargo.toml''"),
        "{text}"
    );
    assert!(
        text.contains("--manifest-path ''tools/b/Cargo.toml''"),
        "{text}"
    );
    Ok(())
}

#[test]
fn the_group_shares_one_target_directory_and_one_cache_entry() -> TestResult {
    let text = grouped(GROUP)?;
    assert!(
        text.contains("CARGO_TARGET_DIR: '${{ github.workspace }}/target-velnor/tools'"),
        "{text}"
    );
    assert!(
        text.contains("hashFiles(''tools/a/Cargo.lock'', ''tools/b/Cargo.lock'')"),
        "{text}"
    );
    let group = job(&text, "rust-tools");
    assert_eq!(group.matches("id: target_tools\n").count(), 1, "{group}");
    assert_eq!(group.matches("id: sources_tools\n").count(), 1, "{group}");
    Ok(())
}

#[test]
fn every_root_of_the_group_keeps_its_own_selection() -> TestResult {
    let text = grouped(GROUP)?;
    assert!(
        text.contains("needs.rust-plan.outputs.rust_group_tools_mode != ''none''"),
        "{text}"
    );
    for root in ["tools_a", "tools_b"] {
        assert!(
            text.contains(&format!(
                "outputs.rust_{root}_mode || needs.rust-plan.outputs.rust_{root}_mode) != ''none''"
            )),
            "{root}: {text}"
        );
    }
    Ok(())
}

#[test]
fn a_root_in_two_groups_is_refused() -> TestResult {
    let config = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n[[workspace_groups]]\nname = \"more\"\nroots = [\"tools/b\", \"tools/a\"]\n";
    let error = refused(config)?;
    assert!(
        error.contains("a root belongs to at most one group"),
        "{error}"
    );
    Ok(())
}

#[test]
fn an_unknown_root_is_refused() -> TestResult {
    let config =
        "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/missing\"]\n";
    let error = refused(config)?;
    assert!(
        error.contains("which is not a Cargo root of this repository"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_group_of_one_root_is_refused() -> TestResult {
    let config = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\"]\n";
    let error = refused(config)?;
    assert!(
        error.contains("invalid value \"tools\" for workspace_groups.roots"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_group_named_after_a_root_is_refused() -> TestResult {
    let config = "[[workspace_groups]]\nname = \"tools_a\"\nroots = [\"tools/a\", \"tools/b\"]\n";
    let error = refused(config)?;
    assert!(
        error.contains("which is the name of a Cargo root of this repository"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_group_needs_a_helper_that_writes_the_group_mode() -> TestResult {
    let error = refused(GROUP)?;
    assert!(
        error.contains("workspace_groups needs velnor-ci 0.1.96 or later"),
        "{error}"
    );
    Ok(())
}

#[test]
fn only_the_building_steps_of_a_group_set_the_shared_target_directory() -> TestResult {
    let text = grouped(GROUP)?;
    let group = job(&text, "rust-tools");
    let shared = "CARGO_TARGET_DIR: '${{ github.workspace }}/target-velnor/tools'";
    assert!(group.matches(shared).count() > 4, "{group}");
    assert_eq!(
        group.matches("CARGO_TARGET_DIR").count(),
        group.matches(shared).count()
    );
    for line in group.lines().filter(|line| line.contains("cargo-fmt")) {
        let at = group.find(line).ok_or("fmt line vanished")?;
        let step = group[..at].rsplit("\n      - ").next().unwrap_or_default();
        assert!(!step.contains("CARGO_TARGET_DIR"), "{step}");
    }
    let own = job(&text, "rust-root");
    assert!(
        own.contains("CARGO_TARGET_DIR: '${{ github.workspace }}/target'"),
        "{own}"
    );
    assert!(
        !own.contains("CARGO_TARGET_DIR: '${{ github.workspace }}/target-velnor"),
        "{own}"
    );
    Ok(())
}

#[test]
fn each_root_of_a_group_picks_the_ci_profile_of_its_own_nextest_config() -> TestResult {
    let mut files = view(&ROOTS, Some(GROUP.as_bytes()))?;
    crate::pipeline::pin_helper(&mut files, "0.1.96")?;
    let config = RepoPath::new("tools/a/.config/nextest.toml")?;
    files.tracked_paths.insert(config.clone());
    files
        .files
        .insert(config, b"[profile.ci]\nslow-timeout = \"60s\"\n".to_vec());
    let text = workflow(&files)?;
    let group = job(&text, "rust-tools");
    let runs: Vec<&str> = group
        .lines()
        .filter(|line| line.contains("nextest run") && line.contains("--no-fail-fast"))
        .collect();
    assert_eq!(runs.len(), 2, "{group}");
    assert!(
        runs[0].contains("tools/a/Cargo.toml") && runs[0].contains("--profile ci"),
        "{group}"
    );
    assert!(
        runs[1].contains("tools/b/Cargo.toml") && !runs[1].contains("--profile ci"),
        "{group}"
    );
    Ok(())
}
