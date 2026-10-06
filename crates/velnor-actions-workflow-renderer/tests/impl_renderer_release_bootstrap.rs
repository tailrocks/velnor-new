//! Release bootstrap identity and ordering regressions.
use velnor_actions_contract::{
    HelperInvocation, SourceBoundHelper, SourceBoundOperation, StepKind,
};
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::{RenderError, SETUP_MISE_NAME};

use super::{ReleaseRole, ReleaseWorkflowSpec, gated_spec, invalid, job_steps, with_steps};

const ROUTINE_JOBS: [&str; 7] = [
    "release-package",
    "release-preparation-source",
    "release-preflight",
    "release-registry-publish",
    "release-forge-publish",
    "release-reconcile",
    "release-preparation",
];

fn setup_index(steps: &[velnor_actions_contract::Step]) -> usize {
    steps
        .iter()
        .position(|step| step.name == SETUP_MISE_NAME)
        .expect("Mise bootstrap")
}

fn mutate_setup(
    workflow: ReleaseWorkflowSpec,
    job_id: &str,
    mutate: impl FnOnce(&mut velnor_actions_contract::Step),
) -> ReleaseWorkflowSpec {
    let mut steps = job_steps(&workflow, job_id).expect("release job");
    let index = setup_index(&steps);
    mutate(&mut steps[index]);
    with_steps(workflow, job_id, steps).expect("release job")
}

#[test]
fn checkout_authority_is_exact_on_anonymous_jobs() -> Result<(), RenderError> {
    for job_id in ["release-package", "release-preparation-source"] {
        let workflow = gated_spec()?;
        let mut steps = job_steps(&workflow, job_id).expect("anonymous job");
        for step in &mut steps {
            if let StepKind::Action { uses, .. } = &mut step.kind {
                *uses = format!("actions/checkout@{}", "f".repeat(40));
            }
        }
        let workflow = with_steps(workflow, job_id, steps).expect("anonymous job");
        assert!(invalid(check_release_jobs(&workflow)).is_some(), "{job_id}");
    }
    Ok(())
}

#[test]
fn bootstrap_record_environment_cannot_be_substituted() -> Result<(), RenderError> {
    for job_id in ROUTINE_JOBS {
        let workflow = mutate_setup(gated_spec()?, job_id, |step| {
            let StepKind::SourceBoundHelper { env, .. } = &mut step.kind else {
                panic!("Mise bootstrap helper")
            };
            env.insert("VELNOR_MISE_VERSION".to_owned(), "2026.9.17".to_owned());
        });
        assert!(invalid(check_release_jobs(&workflow)).is_some(), "{job_id}");
    }
    Ok(())
}

#[test]
fn bootstrap_record_source_and_invocation_are_sealed() -> Result<(), RenderError> {
    let source = mutate_setup(gated_spec()?, "release-preflight", |step| {
        let StepKind::SourceBoundHelper { invocation, .. } = &mut step.kind else {
            panic!("Mise bootstrap helper")
        };
        *invocation = HelperInvocation::compiled(
            SourceBoundHelper::compiled(
                SourceBoundOperation::MiseBootstrap,
                SourceBoundOperation::MiseBootstrap.path(),
                &"b".repeat(64),
            )
            .expect("replacement descriptor"),
            invocation.args().to_vec(),
            invocation.installed_selectors().to_vec(),
        )
        .expect("replacement invocation");
    });
    assert!(invalid(check_release_jobs(&source)).is_some());

    let invocation = mutate_setup(gated_spec()?, "release-preflight", |step| {
        let StepKind::SourceBoundHelper { invocation, .. } = &mut step.kind else {
            panic!("Mise bootstrap helper")
        };
        *invocation = HelperInvocation::compiled(
            invocation.descriptor().clone(),
            vec!["forged".to_owned()],
            invocation.installed_selectors().to_vec(),
        )
        .expect("replacement invocation");
    });
    assert!(invalid(check_release_jobs(&invocation)).is_some());
    Ok(())
}

#[test]
fn bootstrap_must_exist_once_and_before_other_helpers() -> Result<(), RenderError> {
    for job_id in ROUTINE_JOBS {
        let workflow = gated_spec()?;
        let steps = job_steps(&workflow, job_id).expect("release job");
        let index = setup_index(&steps);
        let missing = with_steps(
            workflow.clone(),
            job_id,
            steps
                .iter()
                .enumerate()
                .filter(|(position, _)| *position != index)
                .map(|(_, step)| step.clone())
                .collect(),
        )
        .expect("release job");
        assert!(
            invalid(check_release_jobs(&missing)).is_some(),
            "missing {job_id}"
        );

        let mut duplicate_steps = job_steps(&workflow, job_id).expect("release job");
        let setup = duplicate_steps[index].clone();
        duplicate_steps.insert(index + 1, setup.clone());
        let duplicate = with_steps(workflow.clone(), job_id, duplicate_steps).expect("release job");
        assert!(
            invalid(check_release_jobs(&duplicate)).is_some(),
            "duplicate {job_id}"
        );

        let mut late_steps = job_steps(&workflow, job_id).expect("release job");
        late_steps.remove(index);
        late_steps.push(setup);
        let late = with_steps(workflow, job_id, late_steps).expect("release job");
        assert!(
            invalid(check_release_jobs(&late)).is_some(),
            "late {job_id}"
        );
    }
    Ok(())
}

#[test]
fn helper_registry_requires_one_exact_bootstrap_record() -> Result<(), RenderError> {
    let mut missing = gated_spec()?;
    missing.helper_registry.clear();
    assert!(invalid(check_release_jobs(&missing)).is_some());

    let mut duplicate = gated_spec()?;
    let record = duplicate.helper_registry[0].clone();
    duplicate.helper_registry.push(record);
    assert!(invalid(check_release_jobs(&duplicate)).is_some());

    let mut substituted = gated_spec()?;
    let record = substituted.helper_registry[0].clone();
    substituted.helper_registry[0] = record.with_environment(std::collections::BTreeMap::from([(
        "VELNOR_MISE_VERSION".to_owned(),
        "2026.9.17".to_owned(),
    )]));
    assert!(invalid(check_release_jobs(&substituted)).is_some());
    Ok(())
}

#[test]
fn bootstrap_approval_requires_qualified_runner_mapping() -> Result<(), RenderError> {
    let mut workflow = gated_spec()?;
    workflow
        .jobs
        .get_mut(ReleaseRole::PreflightForge.job_id())
        .expect("preflight")
        .runs_on = "ubuntu-24.04".to_owned();
    assert!(check_release_jobs(&workflow).is_err());
    Ok(())
}
