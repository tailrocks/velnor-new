//! Release helper order, action closure, and anonymous checkout regressions.
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::{RenderError, SETUP_MISE_NAME};

use super::{SHA, fixture, gated_spec, invalid, job_steps, with_steps};
use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;

#[test]
fn source_helpers_are_the_only_release_execution_steps() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    assert!(check_release_jobs(&workflow).is_ok());
    for (id, job) in &workflow.jobs {
        assert!(
            job.steps.iter().all(|step| {
                matches!(
                    &step.kind,
                    StepKind::SourceBoundHelper { .. } | StepKind::Action { .. }
                )
            }),
            "raw execution in {id}"
        );
    }
    Ok(())
}

#[test]
fn source_helpers_follow_the_exact_role_order() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    for (id, role) in [
        ("release-package", ReleaseRole::PackageAnonymous),
        ("release-preflight", ReleaseRole::PreflightForge),
        ("release-registry-publish", ReleaseRole::RegistryPublishOidc),
        ("release-forge-publish", ReleaseRole::ForgePublish),
        ("release-reconcile", ReleaseRole::Reconcile),
        (
            "release-preparation-source",
            ReleaseRole::PreparationAnonymous,
        ),
        ("release-preparation", ReleaseRole::PreparationForge),
    ] {
        let actual = job_steps(&workflow, id)
            .expect("release job")
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::SourceBoundHelper { invocation, .. } => {
                    Some(invocation.descriptor().operation())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, fixture::operation_order(role), "{id}");
    }
    Ok(())
}

#[test]
fn raw_shell_and_credentialed_checkout_are_rejected() -> Result<(), RenderError> {
    let shell = fixture::add_raw_shell(gated_spec()?, "release-preflight", "forged shell");
    assert!(invalid(check_release_jobs(&shell)).is_some());
    let checkout = fixture::add_raw_checkout(gated_spec()?, "release-preflight")?;
    assert!(invalid(check_release_jobs(&checkout)).is_some());
    Ok(())
}

#[test]
fn conditioned_source_helpers_are_rejected() -> Result<(), RenderError> {
    let workflow = fixture::conditioned(
        gated_spec()?,
        "release-preflight",
        "Prove immutable release package",
    );
    assert!(invalid(check_release_jobs(&workflow)).is_some());
    Ok(())
}

#[test]
fn checkout_conditions_and_reordered_admission_are_rejected() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    let mut package_steps = job_steps(&workflow, "release-package").expect("package job");
    package_steps
        .iter_mut()
        .find(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@"))
        })
        .expect("source checkout")
        .condition = Some("false".to_owned());
    let checkout = with_steps(workflow, "release-package", package_steps).expect("package job");
    assert!(invalid(check_release_jobs(&checkout)).is_some());

    let workflow = gated_spec()?;
    let mut reconcile_steps = job_steps(&workflow, "release-reconcile").expect("reconcile job");
    let admission = reconcile_steps
        .iter()
        .position(|step| step.name == "Admit protected CI candidate")
        .expect("admission");
    let admission_step = reconcile_steps.remove(admission);
    reconcile_steps.push(admission_step);
    let reordered =
        with_steps(workflow, "release-reconcile", reconcile_steps).expect("reconcile job");
    assert!(invalid(check_release_jobs(&reordered)).is_some());
    Ok(())
}

#[test]
fn preparation_flag_adds_only_the_two_preparation_roles() -> Result<(), RenderError> {
    let without = super::anonymous_spec()?;
    assert!(!without.preparation_enabled);
    assert!(!without.jobs.contains_key("release-preparation-source"));
    assert!(!without.jobs.contains_key("release-preparation"));
    let with = gated_spec()?;
    assert!(with.preparation_enabled);
    assert!(with.jobs.contains_key("release-preparation-source"));
    assert!(with.jobs.contains_key("release-preparation"));
    Ok(())
}

fn assert_anonymous_checkout(workflow: &ReleaseWorkflowSpec, id: &str) {
    let steps = job_steps(workflow, id).expect("anonymous job");
    let checkouts = steps
        .iter()
        .filter(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@"))
        })
        .collect::<Vec<_>>();
    assert_eq!(checkouts.len(), 1, "{id}");
    let StepKind::Action { with, env, .. } = &checkouts[0].kind else {
        unreachable!("checkout")
    };
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(fixture::source_dir())
    );
    assert_eq!(with.get("ref").map(String::as_str), Some(SHA));
    assert_eq!(
        with.get("persist-credentials").map(String::as_str),
        Some("false")
    );
    assert_eq!(with.get("fetch-depth").map(String::as_str), Some("0"));
    assert!(env.is_empty());
    let checkout_index = steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@"))
        })
        .expect("index");
    let bootstrap_index = steps
        .iter()
        .position(|step| step.name == SETUP_MISE_NAME)
        .expect("bootstrap");
    assert!(checkout_index < bootstrap_index, "{id}");
}

fn assert_no_checkout(workflow: &ReleaseWorkflowSpec, id: &str) {
    let steps = job_steps(workflow, id).expect("credentialed job");
    assert!(
        !steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/checkout@"))
        }),
        "{id}"
    );
}

#[test]
fn anonymous_checkouts_are_exactly_one_before_bootstrap() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    for id in ["release-package", "release-preparation-source"] {
        assert_anonymous_checkout(&workflow, id);
    }
    for id in [
        "release-preflight",
        "release-registry-publish",
        "release-forge-publish",
        "release-reconcile",
    ] {
        assert_no_checkout(&workflow, id);
    }
    Ok(())
}
