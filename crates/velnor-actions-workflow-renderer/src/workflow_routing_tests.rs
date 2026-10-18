use std::collections::BTreeMap;

use velnor_actions_contract::{ExecutionMode, Job, JobTimeout, ScaleSetSelector};

use super::{WorkflowRoutingPolicy, route_jobs, route_workflow_yaml};
use crate::{RenderError, yaml::Yaml};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn policy(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::from_mode(mode, selector())
}

fn workflow(jobs: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(
            jobs.into_iter()
                .map(|(id, job)| (id.to_owned(), job))
                .collect(),
        ),
    )])
}

fn runner_job(runs_on: &str) -> Yaml {
    Yaml::Map(vec![("runs-on".to_owned(), Yaml::str(runs_on))])
}

#[test]
fn hosted_and_both_preserve_existing_placement() {
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        let input = workflow(vec![("build", runner_job("ubuntu-26.04"))]);
        assert_eq!(
            route_workflow_yaml("ci.yml", input.clone(), &policy(mode)),
            Ok(input)
        );
    }
}

#[test]
fn linux_amd64_runner_is_routed_to_the_validated_selector() {
    let input = workflow(vec![("build", runner_job("ubuntu-24.04"))]);
    let expected = workflow(vec![(
        "build",
        Yaml::Map(vec![(
            "runs-on".to_owned(),
            Yaml::Flow(selector().labels().to_vec()),
        )]),
    )]);
    assert_eq!(
        route_workflow_yaml("release.yml", input, &policy(ExecutionMode::ScaleSet)),
        Ok(expected)
    );
}

#[test]
fn non_linux_and_unknown_runner_labels_fail_closed() {
    for (runner, expected) in [
        (
            "macos-26",
            "unsupported_darwin_workload:ci.yml:build:macos-26",
        ),
        (
            "windows-2025",
            "unsupported_workload_for_linux_amd64_scale_set:ci.yml:build:windows-2025",
        ),
    ] {
        let error = route_workflow_yaml(
            "ci.yml",
            workflow(vec![("build", runner_job(runner))]),
            &policy(ExecutionMode::ScaleSet),
        )
        .expect_err("unsupported workload must not be rewritten to Linux");
        assert_eq!(error, RenderError::InvalidWorkflow(expected.to_owned()));
    }
}

#[test]
fn typed_policy_routes_late_added_jobs_and_rejects_darwin() {
    let mut jobs = BTreeMap::from([("alint".to_owned(), job("ubuntu-26.04"))]);
    route_jobs(&mut jobs, &policy(ExecutionMode::ScaleSet))
        .expect("late support jobs use the selected Linux Scale Set");
    assert_eq!(jobs["alint"].runs_on, selector().token());

    let mut darwin = BTreeMap::from([("native".to_owned(), job("macos-15"))]);
    let error = route_jobs(&mut darwin, &policy(ExecutionMode::ScaleSet))
        .expect_err("a Darwin job cannot be assigned to the Linux Scale Set");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_darwin_workload:.github/workflows/ci.yml:native:macos-15".to_owned()
        )
    );
}

fn job(runs_on: &str) -> Job {
    Job {
        display_name: "Runner routing fixture".to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: Vec::new(),
    }
}

#[test]
fn external_reusable_jobs_fail_closed_for_scale_set_routing() {
    let job = Yaml::Map(vec![(
        "uses".to_owned(),
        Yaml::str("owner/repository/.github/workflows/check.yml@0123456789abcdef"),
    )]);
    let error = route_workflow_yaml(
        "ci.yml",
        workflow(vec![("delegate", job)]),
        &policy(ExecutionMode::ScaleSet),
    )
    .expect_err("external workflow has unresolved placement");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "unsupported_external_reusable_workflow:ci.yml:delegate:owner/repository/.github/workflows/check.yml@0123456789abcdef".to_owned()
        )
    );
}
