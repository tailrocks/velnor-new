use std::collections::BTreeMap;

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::{ExecutionMode, Job, JobTimeout, ScaleSetSelector};

use super::{WorkflowRoutingPolicy, route_jobs, route_workflow_yaml};
use crate::{RenderError, yaml::Yaml};

pub(super) fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn policy(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    configured(mode)
}

pub(super) fn image() -> ValidatedImageReference {
    ValidatedImageReference::parse(
        "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid immutable test image")
}

pub(super) fn configured(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::configured(mode, selector(), Some(image()))
}

pub(super) fn workflow(jobs: Vec<(&str, Yaml)>) -> Yaml {
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
fn linux_amd64_runner_is_routed_to_the_validated_selector() {
    let input = workflow(vec![("build", runner_job("ubuntu-24.04"))]);
    let expected = workflow(vec![(
        "build",
        Yaml::Map(vec![
            (
                "runs-on".to_owned(),
                Yaml::Flow(selector().labels().to_vec()),
            ),
            (
                "container".to_owned(),
                Yaml::Map(vec![
                    ("image".to_owned(), Yaml::str(image().as_str())),
                    (
                        "volumes".to_owned(),
                        Yaml::Seq(vec![
                            Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                            Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                        ]),
                    ),
                ]),
            ),
            (
                "defaults".to_owned(),
                Yaml::Map(vec![(
                    "run".to_owned(),
                    Yaml::Map(vec![("shell".to_owned(), Yaml::str("bash -e {0}"))]),
                )]),
            ),
        ]),
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
        services: Vec::new(),
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
fn hosted_only_rejects_typed_scale_set_jobs() {
    let mut jobs = BTreeMap::from([("build".to_owned(), job(&selector().token()))]);
    let error = route_jobs(&mut jobs, &WorkflowRoutingPolicy::hosted_only())
        .expect_err("hosted-only policy cannot return a Scale Set job");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(
            "scale_set_selector_unconfigured:.github/workflows/ci.yml:build".to_owned()
        )
    );
}

#[test]
fn typed_linux_check_already_routed_to_selector_is_accepted() {
    let mut check = job(&selector().token());
    check.condition = Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
    check.check_runner = Some(CheckRunner {
        label: "linux-check".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    });
    let mut jobs = BTreeMap::from([("check-linux".to_owned(), check)]);

    route_jobs(&mut jobs, &policy(ExecutionMode::ScaleSet))
        .expect("the selector is classified before the typed runner");
    assert_eq!(jobs["check-linux"].runs_on, selector().token());
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

#[test]
fn selector_leaf_gets_a_container_in_every_configured_mode() {
    let original = workflow(vec![(
        "build",
        Yaml::Map(vec![(
            "runs-on".to_owned(),
            Yaml::Flow(selector().labels().to_vec()),
        )]),
    )]);
    for mode in [
        ExecutionMode::Hosted,
        ExecutionMode::ScaleSet,
        ExecutionMode::Both,
    ] {
        let routed = route_workflow_yaml("ci.yml", original.clone(), &configured(mode))
            .expect("selector leaves use the configured immutable image");
        let Yaml::Map(doc) = routed else {
            panic!("workflow map")
        };
        let Some((_, Yaml::Map(jobs))) = doc.iter().find(|(key, _)| key == "jobs") else {
            panic!("jobs map")
        };
        let Some((_, Yaml::Map(fields))) = jobs.first() else {
            panic!("job map")
        };
        assert!(fields.iter().any(|(key, value)| {
            key == "container"
                && *value
                    == Yaml::Map(vec![
                        ("image".to_owned(), Yaml::str(image().as_str())),
                        (
                            "volumes".to_owned(),
                            Yaml::Seq(vec![
                                Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                                Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                            ]),
                        ),
                    ])
        }));
    }
}

#[test]
fn scale_set_routing_injects_image_and_preserves_bash_shell() {
    let routed = route_workflow_yaml(
        "ci.yml",
        workflow(vec![("build", runner_job("ubuntu-26.04"))]),
        &configured(ExecutionMode::ScaleSet),
    )
    .expect("Linux work routes into its immutable job container");
    let Yaml::Map(doc) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = doc.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    assert!(fields.iter().any(|(key, value)| {
        key == "runs-on" && *value == Yaml::Flow(selector().labels().to_vec())
    }));
    assert!(fields.iter().any(|(key, value)| {
        key == "defaults"
            && *value
                == Yaml::Map(vec![(
                    "run".to_owned(),
                    Yaml::Map(vec![("shell".to_owned(), Yaml::str("bash -e {0}"))]),
                )])
    }));
    assert!(fields.iter().any(|(key, value)| {
        key == "container"
            && value
                == &Yaml::Map(vec![
                    ("image".to_owned(), Yaml::str(image().as_str())),
                    (
                        "volumes".to_owned(),
                        Yaml::Seq(vec![
                            Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                            Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                        ]),
                    ),
                ])
    }));
}

#[test]
fn configured_selector_needs_an_image_even_when_container_is_explicit() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector(), None);
    let error = route_workflow_yaml(
        "ci.yml",
        workflow(vec![(
            "build",
            Yaml::Map(vec![
                ("runs-on".to_owned(), Yaml::Flow(selector().labels().to_vec())),
                (
                    "container".to_owned(),
                    Yaml::str("ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                ),
            ]),
        )]),
        &policy,
    )
    .expect_err("configuration must provide the default immutable image");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow("job_container_image_missing:ci.yml:build".to_owned())
    );
}

#[test]
fn explicit_container_fields_are_preserved_and_validated() {
    let container = Yaml::Map(vec![
        (
            "image".to_owned(),
            Yaml::str(
                "ghcr.io/team/job@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
        ),
        (
            "credentials".to_owned(),
            Yaml::Map(vec![(
                "username".to_owned(),
                Yaml::str("${{ secrets.USER }}"),
            )]),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![("MODE".to_owned(), Yaml::str("test"))]),
        ),
        ("ports".to_owned(), Yaml::Seq(vec![Yaml::str("6379")])),
        (
            "volumes".to_owned(),
            Yaml::Seq(vec![Yaml::str("data:/data")]),
        ),
        ("options".to_owned(), Yaml::str("--cpus 2")),
    ]);
    let original = workflow(vec![(
        "build",
        Yaml::Map(vec![
            (
                "runs-on".to_owned(),
                Yaml::Flow(selector().labels().to_vec()),
            ),
            ("container".to_owned(), container.clone()),
        ]),
    )]);
    let routed = route_workflow_yaml("ci.yml", original.clone(), &configured(ExecutionMode::Both))
        .expect("supported explicit container fields remain valid");
    let Yaml::Map(document) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    let Some((_, Yaml::Map(container))) = fields.iter().find(|(key, _)| key == "container") else {
        panic!("container map")
    };
    assert!(container.iter().any(|(key, value)| {
        key == "volumes"
            && value
                == &Yaml::Seq(vec![
                    Yaml::str("data:/data"),
                    Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                    Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                ])
    }));
    assert!(
        container
            .iter()
            .any(|(key, value)| key == "options" && value == &Yaml::str("--cpus 2"))
    );
    let _ = original;
}

#[test]
fn reusable_caller_container_stays_invalid_in_hosted_only_mode() {
    let caller = Yaml::Map(vec![
        (
            "uses".to_owned(),
            Yaml::str("./.github/workflows/reuse.yml"),
        ),
        ("container".to_owned(), Yaml::str("ubuntu:latest")),
    ]);
    assert!(
        route_workflow_yaml(
            "ci.yml",
            workflow(vec![("caller", caller)]),
            &WorkflowRoutingPolicy::hosted_only(),
        )
        .is_err()
    );
}

#[path = "workflow_routing_policy_tests.rs"]
mod policy_tests;
