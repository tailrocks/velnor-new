// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use super::reuse_verdict::{KNOB, REUSED, condition, jobs, step};
use crate::facts::TestResult;
use crate::lint_lane::{LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};

fn renamed(kind: &str, name: &str, knob: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let marker = format!("kind = '{kind}'\n");
    let lint = LINT.replacen(&marker, &format!("{marker}name = '{name}'\n"), 1);
    let config = format!("{}{lint}", if knob { KNOB } else { "" });
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, "0.1.117")?;
    Ok(repo)
}

#[test]
fn a_lint_job_named_like_another_job_or_job_id_is_refused_when_reuse_is_on() -> TestResult {
    for (kind, name) in [
        ("jactionlint", "rust-root"),
        ("jactionlint", "Alint"),
        ("jactionlint", "lint-alint"),
    ] {
        let error = workflow(&renamed(kind, name, true)?)
            .err()
            .ok_or("a job shared its name with another job")?
            .to_string();
        assert!(
            error.contains(&format!("the job \"{name}\" shares its name")),
            "{name}: {error}"
        );
    }
    Ok(())
}

#[test]
fn a_shared_job_name_is_harmless_without_reuse_and_a_distinct_name_passes() -> TestResult {
    workflow(&renamed("jactionlint", "rust-root", false)?)?;
    workflow(&renamed("jactionlint", "Action lint", true)?)?;
    Ok(())
}

#[test]
fn the_per_root_audit_is_reusable_in_the_event_mode_that_runs_in_both_runs() -> TestResult {
    let all = jobs(&["Cargo.toml", "lab/Cargo.toml"])?;
    let root = &all["rust-root"];
    let name = "velnor-ci inputs audit --root .";
    assert!(
        condition(root, name)?.contains(&format!("!{REUSED}rust-root.inputs-audit-1 ')")),
        "{}",
        condition(root, name)?
    );
    let list = step(&all["rust-plan"], "velnor-ci verdict reuse")?["env"]["VELNOR_REUSABLE"]
        .as_str()
        .ok_or("no list")?
        .to_owned();
    let items: Vec<Value> = serde_json::from_str(&list)?;
    assert!(
        items
            .iter()
            .any(|item| item["id"] == "rust-root.inputs-audit-1"
                && item["job"] == "rust-root"
                && item["step"] == name),
        "{list}"
    );
    Ok(())
}

fn audit_condition(on_main: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let config = format!("on_main = '{on_main}'\n{KNOB}{LINT}");
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, "0.1.117")?;
    let document = crate::yaml_tree::document(&workflow(&repo)?)?;
    let all = &document["jobs"];
    let list = step(&all["rust-plan"], "velnor-ci verdict reuse")?["env"]["VELNOR_REUSABLE"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    Ok((
        condition(&all["rust-root"], "velnor-ci inputs audit --root .")?,
        list,
    ))
}

#[test]
fn the_per_root_audit_gets_its_id_in_every_event_mode() -> TestResult {
    let id = "rust-root.inputs-audit-1";
    for on_main in ["affected", "everything"] {
        let (condition, list) = audit_condition(on_main)?;
        assert!(
            condition.contains(&format!("!{REUSED}{id} ')")),
            "{on_main}: {condition}"
        );
        assert!(list.contains(id), "{on_main}: {list}");
    }
    Ok(())
}
