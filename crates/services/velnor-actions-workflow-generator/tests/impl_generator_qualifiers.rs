//! Qualifier jobs fetch exact source, then qualify with pinned Mise.

use super::impl_generator_mise_setup::{
    assert_guard_runs_after_step, assert_setup_step, distinct_pins, has_field, map_entries,
    map_field, qualifier_cases, render, scalar, sequence, workflow_jobs,
};
use std::error::Error;
use velnor_actions_workflow_generator::generator_release::GeneratorRelease;
use velnor_actions_workflow_steps::setup::MiseSetup;
use velnor_actions_workflow_tree::yaml::Yaml;

#[test]
fn linux_qualifier_fetches_source_then_qualifies() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let (job_id, action, target) = qualifier_cases()[0];
    assert_qualifier_setup(
        &release,
        workflow_jobs(&release)?,
        job_id,
        action,
        pins.setup_for(target),
    )?;
    Ok(())
}

#[test]
fn macos_qualifier_fetches_source_then_qualifies() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let (job_id, action, target) = qualifier_cases()[1];
    assert_qualifier_setup(
        &release,
        workflow_jobs(&release)?,
        job_id,
        action,
        pins.setup_for(target),
    )?;
    Ok(())
}

#[test]
fn macos_intel_qualifier_fetches_source_then_qualifies() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let (job_id, action, target) = qualifier_cases()[2];
    assert_qualifier_setup(
        &release,
        workflow_jobs(&release)?,
        job_id,
        action,
        pins.setup_for(target),
    )?;
    Ok(())
}

#[test]
fn qualifiers_never_use_the_default_checkout_action() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let jobs = workflow_jobs(&release)?;
    for (job_id, _, _) in qualifier_cases() {
        let job = map_field(jobs, job_id)?;
        let steps = sequence(map_field(map_entries(job)?, "steps")?)?;
        assert_no_default_checkout(steps, job_id)?;
    }
    Ok(())
}

#[test]
fn qualifiers_retain_empty_job_permissions() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let jobs = workflow_jobs(&release)?;
    for (job_id, _, _) in qualifier_cases() {
        assert_empty_permissions(jobs, job_id)?;
    }
    Ok(())
}

fn assert_empty_permissions(jobs: &[(String, Yaml)], job_id: &str) -> Result<(), Box<dyn Error>> {
    let job = map_field(jobs, job_id)?;
    assert_eq!(
        map_entries(map_field(map_entries(job)?, "permissions")?)?.len(),
        0,
        "{job_id} must retain empty job permissions"
    );
    Ok(())
}

fn assert_qualifier_setup(
    release: &GeneratorRelease,
    jobs: &[(String, Yaml)],
    job_id: &str,
    action: &str,
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let job = map_field(jobs, job_id)?;
    let steps = sequence(map_field(map_entries(job)?, "steps")?)?;
    assert_eq!(steps.len(), 2, "{job_id} must fetch source, then qualify");
    assert_source_step(&steps[0], job_id)?;
    assert_qualifier_step(&steps[1], action, job_id)?;
    assert_action_extracts_guard(&release.actions, action, setup, job_id)?;
    Ok(())
}

fn assert_action_extracts_guard(
    actions: &[(String, Yaml)],
    action: &str,
    setup: &MiseSetup,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let path = format!("{}/action.yml", action.trim_start_matches("./"));
    let (_, document) = actions
        .iter()
        .find(|(candidate, _)| candidate == &path)
        .ok_or_else(|| format!("missing generated action {path}"))?;
    let runs = map_entries(map_field(map_entries(document)?, "runs")?)?;
    let steps = sequence(map_field(runs, "steps")?)?;
    assert_setup_step(
        steps.first().ok_or("qualifier action has no steps")?,
        setup,
        job_id,
    )?;
    assert_guard_runs_after_step(steps, 0, job_id);
    assert!(
        steps.iter().any(|step| {
            map_entries(step)
                .ok()
                .and_then(|fields| map_field(fields, "run").ok())
                .and_then(|run| scalar(run).ok())
                .is_some_and(|run| run.contains("with-owned-archive-guard.sh"))
        }),
        "{job_id} action must use the owned archive guard"
    );
    Ok(())
}

fn assert_source_step(step: &Yaml, job_id: &str) -> Result<(), Box<dyn Error>> {
    let source = map_entries(step)?;
    assert_eq!(scalar(map_field(source, "shell")?)?, "bash", "{job_id}");
    let source_run = scalar(map_field(source, "run")?)?;
    assert!(
        !has_field(source, "uses"),
        "{job_id} must acquire source with an inline executable step"
    );
    assert!(
        source_run.contains("tailrocks/velnor-new")
            && source_run.contains("https://github.com/{repository}.git")
            && source_run.contains("GITHUB_REPOSITORY")
            && source_run.contains("GITHUB_SHA")
            && source_run.contains(r#"r"[0-9a-f]{40}""#)
            && source_run.contains("--depth=1")
            && source_run.contains("--no-tags")
            && source_run
                .contains("run_git(\"checkout\", \"--quiet\", \"--detach\", \"FETCH_HEAD\"")
            && source_run.contains("head != commit"),
        "{job_id} must validate, fetch, and verify the exact public source commit"
    );
    for credential in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "MISE_GITHUB_TOKEN",
    ] {
        assert!(
            source_run.contains(&format!("\"{credential}\""))
                && source_run.contains("environment.pop(name, None)"),
            "{job_id} must scrub {credential} before fetching source"
        );
    }
    assert!(
        !has_field(source, "env"),
        "{job_id} source acquisition must not receive a credential env"
    );
    Ok(())
}

fn assert_qualifier_step(
    step: &Yaml,
    expected_action: &str,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let qualification = map_entries(step)?;
    assert_eq!(
        scalar(map_field(qualification, "uses")?)?,
        expected_action,
        "{job_id}"
    );
    Ok(())
}

fn assert_no_default_checkout(steps: &[Yaml], job_id: &str) -> Result<(), Box<dyn Error>> {
    for step in steps {
        let fields = map_entries(step)?;
        if let Some((_, uses)) = fields.iter().find(|(key, _)| key == "uses") {
            assert!(
                !scalar(uses)?.starts_with("actions/checkout@"),
                "{job_id} must not depend on the default checkout action"
            );
        }
    }
    Ok(())
}
