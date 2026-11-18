// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::GroupEvent;
use velnor_rust::detect;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::{DATABASE, SHELL, job, rejection, rendered, steps, view_with};
use crate::yaml_tree::document;

const SELECTED: &str =
    "paths = ['app/migrations/**', 'app/seed.sql']\nevents = ['push', 'schedule']\n";

fn selected() -> String {
    format!("{DATABASE}{SELECTED}\n{SHELL}")
}

fn condition(workflow: &serde_json::Value, id: &str) -> Result<String, String> {
    job(workflow, id)?["if"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{id} has no if"))
}

#[test]
fn a_group_takes_paths_and_events() -> TestResult {
    let facts = detect(&view_with(&["Cargo.toml"], &selected(), "0.1.82")?)?;
    let group = &facts.config.test_groups[0];
    assert_eq!(
        group
            .paths
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["app/migrations/**", "app/seed.sql"]
    );
    assert_eq!(group.events, [GroupEvent::Push, GroupEvent::Schedule]);
    let shell = &facts.config.test_groups[1];
    assert!(shell.paths.is_empty() && shell.events.is_empty());
    Ok(())
}

#[test]
fn an_unknown_event_or_a_bad_path_is_refused_with_its_value() -> TestResult {
    for (extra, mention) in [
        ("events = ['monday']\n", "monday"),
        ("paths = ['../escape']\n", "../escape"),
    ] {
        let message = rejection(&format!("{DATABASE}root = '.'\n{extra}"))?;
        assert!(message.contains(mention), "{message}");
    }
    Ok(())
}

#[test]
fn a_group_with_a_selection_runs_only_when_the_plan_says_so_and_the_others_always_run() -> TestResult
{
    let workflow = rendered(&selected())?;
    let database = condition(&workflow, "rust-test-database")?;
    assert!(
        database.contains("needs.rust-plan.outputs.rust_root_group_database == 'run'")
            && database.contains("needs.rust-plan.outputs.rust_root_mode != 'none'"),
        "{database}"
    );
    for always in ["rust-test", "rust-test-default", "rust-test-shell"] {
        let text = condition(&workflow, always)?;
        assert!(!text.contains("_group_"), "{always}: {text}");
    }
    let outputs = &job(&workflow, "rust-plan")?["outputs"];
    assert!(outputs["rust_root_group_database"].is_string(), "{outputs}");
    assert!(outputs["rust_root_group_shell"].is_null(), "{outputs}");
    Ok(())
}

#[test]
fn the_required_gate_expects_a_skipped_group_exactly_when_the_plan_skips_it() -> TestResult {
    let text = workflow(&view_with(&["Cargo.toml"], &selected(), "0.1.82")?)?;
    let spec = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("GATE_SPEC: '"))
        .ok_or("no gate spec")?;
    assert!(
        spec.contains("--job rust-test-database=lane:root+group:root:database"),
        "{spec}"
    );
    assert!(spec.contains("--job rust-test-shell=lane:root "), "{spec}");
    assert!(
        spec.contains("--job rust-test-default=lane:root "),
        "{spec}"
    );
    Ok(())
}

#[test]
fn a_group_of_another_root_reads_that_roots_output() -> TestResult {
    let config = "[[test_groups]]\nname = 'slow'\nroot = 'lab'\nfilter = 'all()'\npaths = ['lab/slow/**']\n\n[[test_groups]]\nname = 'top'\nroot = '.'\nfilter = 'all()'\nevents = ['schedule']\n";
    let text = workflow(&view_with(
        &["Cargo.toml", "lab/Cargo.toml"],
        config,
        "0.1.82",
    )?)?;
    let tree = document(&text)?;
    let slow = condition(&tree, "rust-test-slow-lab")?;
    assert!(
        slow.contains("rust_lab_group_slow == 'run'") && slow.contains("rust_lab_mode != 'none'"),
        "{slow}"
    );
    let top = condition(&tree, "rust-test-top-root")?;
    assert!(top.contains("rust_root_group_top == 'run'"), "{top}");
    assert!(!top.contains("rust_lab_"), "{top}");
    let outputs = &job(&tree, "rust-plan")?["outputs"];
    assert!(
        outputs["rust_lab_group_slow"].is_string() && outputs["rust_root_group_top"].is_string()
    );
    let spec = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("GATE_SPEC: '"))
        .ok_or("no gate spec")?;
    assert!(
        spec.contains("--job rust-test-slow-lab=lane:lab+group:lab:slow"),
        "{spec}"
    );
    Ok(())
}

#[test]
fn a_generated_run_stays_one_command_when_groups_select_by_paths_and_events() -> TestResult {
    let workflow = rendered(&selected())?;
    for id in [
        "rust-test",
        "rust-test-default",
        "rust-test-database",
        "rust-test-shell",
    ] {
        for step in steps(job(&workflow, id)?) {
            let Some(run) = step["run"].as_str() else {
                continue;
            };
            for forbidden in ["\n", ";", "|", "&", "${{"] {
                assert!(
                    !run.contains(forbidden),
                    "{id}: {run:?} contains {forbidden:?}"
                );
            }
        }
    }
    Ok(())
}
