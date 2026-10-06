//! Release checkout shape and credential-boundary regressions.
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;

use super::impl_renderer_release_gates::{
    OTHER_SHA, SHA, gated_spec, invalid, job_steps, source_checkout, with_steps,
};

#[test]
fn anonymous_source_checkout_is_exact_and_credential_free() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    for job_id in ["release-package", "release-preparation-source"] {
        let steps = job_steps(&workflow, job_id).expect("anonymous job");
        let checkout = steps
            .iter()
            .find(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")))
            .expect("source checkout");
        let StepKind::Action { with, env, .. } = &checkout.kind else {
            unreachable!("checkout")
        };
        assert_eq!(with.get("path").map(String::as_str), Some("release-source"));
        assert_eq!(with.get("ref").map(String::as_str), Some(SHA));
        assert_eq!(with.get("fetch-depth").map(String::as_str), Some("0"));
        assert_eq!(
            with.get("persist-credentials").map(String::as_str),
            Some("false")
        );
        assert!(env.is_empty());
    }
    assert!(check_release_jobs(&workflow).is_ok());
    Ok(())
}

#[test]
fn anonymous_checkout_rejects_credentials_and_rebound_source() -> Result<(), RenderError> {
    for (key, value) in [("persist-credentials", "true"), ("fetch-depth", "1")] {
        let workflow = gated_spec()?;
        let mut steps = job_steps(&workflow, "release-package").expect("package");
        let checkout = steps
            .iter_mut()
            .find(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")))
            .expect("checkout");
        let StepKind::Action { with, .. } = &mut checkout.kind else {
            unreachable!("checkout")
        };
        with.insert(key.to_owned(), value.to_owned());
        let workflow = with_steps(workflow, "release-package", steps).expect("package");
        assert!(invalid(check_release_jobs(&workflow)).is_some(), "{key}");
    }
    let workflow = gated_spec()?;
    let mut steps = job_steps(&workflow, "release-package").expect("package");
    let checkout = steps
        .iter_mut()
        .find(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")))
        .expect("checkout");
    let StepKind::Action { with, .. } = &mut checkout.kind else {
        unreachable!("checkout")
    };
    with.insert("ref".to_owned(), OTHER_SHA.to_owned());
    let workflow = with_steps(workflow, "release-package", steps).expect("package");
    assert!(invalid(check_release_jobs(&workflow)).is_some());
    Ok(())
}

#[test]
fn anonymous_checkout_rejects_second_checkout_and_extra_inputs() -> Result<(), RenderError> {
    let workflow = gated_spec()?;
    let mut steps = job_steps(&workflow, "release-package").expect("package");
    steps.insert(1, source_checkout(SHA, Some("false"))?);
    let workflow = with_steps(workflow, "release-package", steps).expect("package");
    assert!(invalid(check_release_jobs(&workflow)).is_some());

    let workflow = gated_spec()?;
    let mut steps = job_steps(&workflow, "release-package").expect("package");
    let checkout = steps
        .iter_mut()
        .find(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")))
        .expect("checkout");
    let StepKind::Action { with, .. } = &mut checkout.kind else {
        unreachable!("checkout")
    };
    with.insert("token".to_owned(), "${{ secrets.TOKEN }}".to_owned());
    let workflow = with_steps(workflow, "release-package", steps).expect("package");
    assert!(invalid(check_release_jobs(&workflow)).is_some());
    Ok(())
}

#[test]
fn credentialed_roles_have_no_checkout_escape_hatch() -> Result<(), RenderError> {
    for job_id in [
        "release-preflight",
        "release-registry-publish",
        "release-forge-publish",
        "release-reconcile",
        "release-preparation",
    ] {
        let workflow = gated_spec()?;
        let steps = job_steps(&workflow, job_id).expect("credentialed job");
        let checkout = source_checkout(SHA, Some("false"))?;
        let mut mutated = steps;
        mutated.insert(0, checkout);
        let workflow = with_steps(workflow, job_id, mutated).expect("credentialed job");
        assert!(invalid(check_release_jobs(&workflow)).is_some(), "{job_id}");
    }
    Ok(())
}
