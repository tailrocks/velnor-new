// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::job_shape::with_targets;
use crate::pipeline::{view, workflow};
use crate::select::facts;
use velnor_model::{CargoCommand, DocsRun, StepKind, TargetKind, When};
use velnor_rust::plan;

const POST_MERGE: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";

fn step_conditions(
    config: &str,
    named: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let conditions: Vec<String> = jobs
        .values()
        .flat_map(|job| job["steps"].as_array().into_iter().flatten())
        .filter(|step| named(step["name"].as_str().unwrap_or_default()))
        .map(|step| step["if"].as_str().unwrap_or_default().to_owned())
        .collect();
    Ok(conditions)
}

fn documentation_conditions(config: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    step_conditions(config, &|name| name == "Documentation")
}

fn alone_whens(mode: Option<&str>) -> Vec<When> {
    let members = vec![with_targets("lib", &[TargetKind::Lib])];
    let mut facts = facts(vec![("Cargo.toml", members)]);
    if let Some(mode) = mode {
        facts.config.check_alone_run = if mode == "post_merge" {
            DocsRun::PostMerge
        } else {
            DocsRun::Always
        };
    }
    plan(&facts)
        .jobs
        .iter()
        .flat_map(|job| job.steps.iter())
        .filter(|step| matches!(step.kind, StepKind::Cargo(CargoCommand::CheckAlone { .. })))
        .map(|step| step.when)
        .collect()
}

#[test]
fn check_alone_runs_on_every_event_unless_configured() {
    for mode in [None, Some("always")] {
        let whens = alone_whens(mode);
        assert_ne!(whens.len(), 0);
        assert!(whens.iter().all(|w| *w == When::NotCancelled));
    }
}

#[test]
fn post_merge_check_alone_defers_only_the_check_alone_steps() {
    let whens = alone_whens(Some("post_merge"));
    assert_ne!(whens.len(), 0);
    assert!(whens.iter().all(|w| *w == When::NotCancelledPostMerge));
}

#[test]
fn the_check_alone_key_parses_and_an_unknown_value_is_refused() -> TestResult {
    let rendered = documentation_conditions("check_alone = \"post_merge\"\n")?;
    assert!(rendered.iter().all(|c| !c.contains("github.event_name")));
    assert!(documentation_conditions("check_alone = \"never\"\n").is_err());
    Ok(())
}

#[test]
fn documentation_runs_on_every_event_unless_configured() -> TestResult {
    for config in ["", "docs = \"always\"\n"] {
        let conditions = documentation_conditions(config)?;
        assert_ne!(conditions.len(), 0);
        assert!(conditions.iter().all(|c| !c.contains("github.event_name")));
    }
    Ok(())
}

#[test]
fn post_merge_documentation_skips_pull_requests_and_merge_groups() -> TestResult {
    let conditions = documentation_conditions("docs = \"post_merge\"\n")?;
    assert_ne!(conditions.len(), 0);
    for condition in &conditions {
        assert!(condition.contains(POST_MERGE), "{condition}");
        assert!(condition.contains("!cancelled()"), "{condition}");
    }
    Ok(())
}

#[test]
fn an_unknown_docs_mode_is_refused() {
    assert!(documentation_conditions("docs = \"never\"\n").is_err());
}
