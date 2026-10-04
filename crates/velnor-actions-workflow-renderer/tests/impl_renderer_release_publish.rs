//! Release publication scopes, evidence bindings, and receipt channels.
use velnor_actions_contract::{SourceBoundOperation, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::{
    ARTIFACT_RETENTION_DAYS, RUN_KEY_EXPR, UPLOAD_ARTIFACT_USES,
};

use super::impl_renderer_release_gates::{
    BOOTSTRAP_ENV, ENV, bootstrap_spec, gated_spec, invalid, job_steps, with_steps,
};

fn mutate_helper_env(
    workflow: ReleaseWorkflowSpec,
    job_id: &str,
    operation: SourceBoundOperation,
    key: &str,
    value: &str,
) -> ReleaseWorkflowSpec {
    let mut steps = job_steps(&workflow, job_id).expect("release job");
    let step = steps
        .iter_mut()
        .find(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == operation)
        })
        .expect("release helper");
    let StepKind::SourceBoundHelper { env, .. } = &mut step.kind else {
        unreachable!("matched source helper")
    };
    env.insert(key.to_owned(), value.to_owned());
    with_steps(workflow, job_id, steps).expect("release job")
}

fn helper_env(
    workflow: &ReleaseWorkflowSpec,
    job_id: &str,
    operation: SourceBoundOperation,
) -> std::collections::BTreeMap<String, String> {
    job_steps(workflow, job_id)
        .expect("release job")
        .into_iter()
        .find_map(|step| match step.kind {
            StepKind::SourceBoundHelper { invocation, env }
                if invocation.descriptor().operation() == operation =>
            {
                Some(env)
            }
            _ => None,
        })
        .expect("release helper")
}

#[test]
fn oidc_and_bootstrap_scopes_cannot_mix_token_material() -> Result<(), RenderError> {
    let oidc = mutate_helper_env(
        gated_spec()?,
        "release-registry-publish",
        SourceBoundOperation::RustRegistryPublish,
        "CARGO_REGISTRY_TOKEN",
        "${{ secrets.CARGO_REGISTRY_TOKEN }}",
    );
    assert!(invalid(check_release_jobs(&oidc)).is_some());

    let bootstrap = mutate_helper_env(
        bootstrap_spec()?,
        "release-registry-publish",
        SourceBoundOperation::RustRegistryPublish,
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "${{ env.ACTIONS_ID_TOKEN_REQUEST_URL }}",
    );
    assert!(invalid(check_release_jobs(&bootstrap)).is_some());

    let forge = mutate_helper_env(
        gated_spec()?,
        "release-forge-publish",
        SourceBoundOperation::RustForgePublish,
        "CARGO_REGISTRY_TOKEN",
        "${{ secrets.CARGO_REGISTRY_TOKEN }}",
    );
    assert!(invalid(check_release_jobs(&forge)).is_some());
    Ok(())
}

#[test]
fn proof_policy_source_and_artifact_handles_are_independently_bound() -> Result<(), RenderError> {
    let source = mutate_helper_env(
        gated_spec()?,
        "release-preflight",
        SourceBoundOperation::ReleaseAdmissionDefaultBranch,
        "APPROVED_SOURCE_SHA",
        super::impl_renderer_release_gates::OTHER_SHA,
    );
    assert!(invalid(check_release_jobs(&source)).is_some());

    let policy = mutate_helper_env(
        gated_spec()?,
        "release-preflight",
        SourceBoundOperation::RustReleaseForgePreflight,
        "RELEASE_RECONCILE_POLICY",
        "forged-policy",
    );
    assert!(invalid(check_release_jobs(&policy)).is_some());

    let artifact = mutate_helper_env(
        gated_spec()?,
        "release-registry-publish",
        SourceBoundOperation::RustRegistryArtifactProof,
        "RELEASE_PACKAGE_ARTIFACT_ID",
        "${{ needs.release-forge-publish.outputs.forge-receipt-artifact-id }}",
    );
    assert!(invalid(check_release_jobs(&artifact)).is_some());
    Ok(())
}

fn assert_receipt_channel(
    workflow: &ReleaseWorkflowSpec,
    job_id: &str,
    expected_id: &str,
    prefix: &str,
    path: &str,
    expected_always: bool,
) {
    let steps = job_steps(workflow, job_id).expect("release job");
    let Some(last) = steps.last() else {
        panic!("release channel")
    };
    let StepKind::Action {
        env, uses, with, ..
    } = &last.kind
    else {
        panic!("release channel action")
    };
    assert_eq!(last.id.as_ref().map(|id| id.as_str()), Some(expected_id));
    assert_eq!(
        last.condition.as_deref(),
        expected_always.then_some("always()")
    );
    assert!(env.is_empty());
    assert_eq!(uses, UPLOAD_ARTIFACT_USES);
    let expected_name = format!("velnor-release-{prefix}-{RUN_KEY_EXPR}");
    assert_eq!(with.get("name"), Some(&expected_name));
    assert_eq!(with.get("path").map(String::as_str), Some(path));
    assert_eq!(
        with.get("if-no-files-found").map(String::as_str),
        Some("error")
    );
    assert_eq!(with.get("retention-days").map(String::as_str), Some("30"));
    assert_eq!(ARTIFACT_RETENTION_DAYS, 30);
}

#[test]
fn final_receipt_channels_are_always_and_exactly_role_owned() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    for (job_id, expected_id, prefix, path, expected_always) in [
        (
            "release-package",
            "release-package-artifact",
            "package",
            "release-package",
            false,
        ),
        (
            "release-preflight",
            "release-preflight-artifact",
            "preflight",
            "release-preflight/evidence.json",
            false,
        ),
        (
            "release-registry-publish",
            "release-registry-receipt-artifact",
            "registry",
            "release-registry/receipt.json",
            true,
        ),
        (
            "release-forge-publish",
            "release-forge-receipt-artifact",
            "forge",
            "release-forge/receipt.json",
            true,
        ),
        (
            "release-reconcile",
            "release-reconcile-receipt-artifact",
            "reconcile",
            "release-receipt/receipt.json",
            true,
        ),
        (
            "release-preparation-source",
            "release-proposal-artifact",
            "proposal",
            "release-proposal/evidence.json",
            false,
        ),
        (
            "release-preparation",
            "release-preparation-receipt-artifact",
            "preparation",
            "release-preparation/evidence.json",
            true,
        ),
    ] {
        assert_receipt_channel(
            &workflow,
            job_id,
            expected_id,
            prefix,
            path,
            expected_always,
        );
    }
    Ok(())
}

#[test]
fn bootstrap_preparation_stays_neutral() -> Result<(), RenderError> {
    let workflow = bootstrap_spec()?;
    assert!(check_release_jobs(&workflow).is_ok());
    for job_id in ["release-preparation-source", "release-preparation"] {
        let job = workflow.jobs.get(job_id).expect("preparation job");
        assert_eq!(
            job.environment.as_deref(),
            if job_id == "release-preparation" {
                Some(ENV)
            } else {
                None
            }
        );
        for step in &job.steps {
            if let StepKind::SourceBoundHelper { env, .. } = &step.kind {
                assert!(!env.contains_key("CARGO_REGISTRY_TOKEN"));
                assert!(!env.contains_key("ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
            }
        }
    }
    let publish = helper_env(
        &workflow,
        "release-registry-publish",
        SourceBoundOperation::RustRegistryPublish,
    );
    assert_eq!(
        publish.get("CARGO_REGISTRY_TOKEN").map(String::as_str),
        Some("${{ secrets.CARGO_REGISTRY_TOKEN }}")
    );
    assert_eq!(
        workflow.jobs["release-registry-publish"]
            .environment
            .as_deref(),
        Some(BOOTSTRAP_ENV)
    );
    Ok(())
}
