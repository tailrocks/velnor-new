//! Closed release graph and authority regressions.
use super::{
    BOOTSTRAP_ENV, PUBLISH_ENV, REPO, ReleaseRole, RenderError, bootstrap_spec, invalid, job,
    preparation_spec, spec,
};
use velnor_actions_workflow_renderer::release_spec::{
    publish_gate_condition, reconcile_gate_condition,
};

fn needs(
    workflow: &velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec,
    role: ReleaseRole,
) -> Vec<String> {
    workflow
        .jobs
        .get(role.job_id())
        .expect("role job")
        .needs
        .clone()
}

fn condition(
    workflow: &velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec,
    role: ReleaseRole,
) -> Option<String> {
    workflow
        .jobs
        .get(role.job_id())
        .expect("role job")
        .condition
        .clone()
}

fn assert_exact_edges(
    workflow: &velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec,
) {
    let package = ReleaseRole::PackageAnonymous.job_id().to_owned();
    let preflight = ReleaseRole::PreflightForge.job_id().to_owned();
    let registry = ReleaseRole::RegistryPublishOidc.job_id().to_owned();
    let forge = ReleaseRole::ForgePublish.job_id().to_owned();
    assert_eq!(needs(workflow, ReleaseRole::PackageAnonymous), Vec::new());
    assert_eq!(
        needs(workflow, ReleaseRole::PreflightForge),
        vec![package.clone()]
    );
    assert_eq!(
        needs(workflow, ReleaseRole::RegistryPublishOidc),
        vec![package.clone(), preflight.clone()]
    );
    assert_eq!(
        needs(workflow, ReleaseRole::ForgePublish),
        vec![package.clone(), preflight.clone(), registry.clone()]
    );
    assert_eq!(
        needs(workflow, ReleaseRole::Reconcile),
        vec![package, preflight, registry, forge]
    );
    assert_eq!(
        needs(workflow, ReleaseRole::PreparationAnonymous),
        Vec::new()
    );
    assert_eq!(
        needs(workflow, ReleaseRole::PreparationForge),
        vec![ReleaseRole::PreparationAnonymous.job_id().to_owned()]
    );
}

#[test]
fn closed_graph_has_exact_ids_edges_conditions_and_environments() -> Result<(), RenderError> {
    let workflow = preparation_spec()?;
    assert_exact_edges(&workflow);

    let gate = publish_gate_condition(REPO, &workflow.bootstrap, "main");
    assert_eq!(
        condition(&workflow, ReleaseRole::RegistryPublishOidc),
        Some(gate.clone())
    );
    assert_eq!(
        condition(&workflow, ReleaseRole::ForgePublish),
        Some(gate.clone())
    );
    assert_eq!(
        condition(&workflow, ReleaseRole::PreparationForge),
        Some(gate)
    );
    assert_eq!(
        condition(&workflow, ReleaseRole::Reconcile),
        Some(reconcile_gate_condition(REPO, &workflow.bootstrap, "main"))
    );
    for role in [
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        ReleaseRole::PreparationAnonymous,
    ] {
        assert_eq!(condition(&workflow, role), None);
    }

    for role in [ReleaseRole::RegistryPublishOidc, ReleaseRole::ForgePublish] {
        assert_eq!(
            workflow.jobs[role.job_id()].environment.as_deref(),
            Some(PUBLISH_ENV)
        );
    }
    assert_eq!(
        workflow.jobs[ReleaseRole::PreparationForge.job_id()]
            .environment
            .as_deref(),
        Some(PUBLISH_ENV)
    );
    for role in [
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        ReleaseRole::Reconcile,
        ReleaseRole::PreparationAnonymous,
    ] {
        assert_eq!(workflow.jobs[role.job_id()].environment, None);
    }
    assert!(workflow.validate().is_ok());
    Ok(())
}

#[test]
fn oidc_and_bootstrap_publishers_are_valid_but_exclusive() -> Result<(), RenderError> {
    assert!(spec()?.validate().is_ok());
    assert!(bootstrap_spec()?.validate().is_ok());
    let mut pair = spec()?;
    let gate = publish_gate_condition(REPO, &pair.bootstrap, "main");
    pair.jobs.insert(
        "release-registry-publish-bootstrap".to_owned(),
        job(
            ReleaseRole::RegistryPublishBootstrap,
            &[
                ReleaseRole::PackageAnonymous.job_id(),
                ReleaseRole::PreflightForge.job_id(),
            ],
            Some(&gate),
            Some(BOOTSTRAP_ENV),
        )?,
    );
    assert!(pair.validate().is_err());
    Ok(())
}

#[test]
fn role_set_rejects_missing_roles_and_wrong_ids() -> Result<(), RenderError> {
    let mut missing = spec()?;
    missing.jobs.remove(ReleaseRole::ForgePublish.job_id());
    assert!(invalid(missing.validate()).is_some_and(|text| text.starts_with("release_role_set:")));

    let mut wrong_id = spec()?;
    let job = wrong_id
        .jobs
        .remove(ReleaseRole::PreflightForge.job_id())
        .expect("preflight");
    wrong_id
        .jobs
        .insert("release-preflight-extra".to_owned(), job);
    assert!(
        invalid(wrong_id.validate())
            .is_some_and(|text| text.starts_with("release_producer_identity:"))
    );
    Ok(())
}

#[test]
fn no_ancestor_extra_edge_and_duplicate_edge_are_rejected() -> Result<(), RenderError> {
    let mut no_ancestor = preparation_spec()?;
    no_ancestor
        .jobs
        .get_mut(ReleaseRole::PackageAnonymous.job_id())
        .expect("package")
        .needs = vec![ReleaseRole::PreparationAnonymous.job_id().to_owned()];
    assert!(invalid(no_ancestor.validate()).is_some());

    let mut extra = preparation_spec()?;
    extra
        .jobs
        .get_mut(ReleaseRole::RegistryPublishOidc.job_id())
        .expect("registry")
        .needs
        .push(ReleaseRole::PreparationAnonymous.job_id().to_owned());
    assert!(invalid(extra.validate()).is_some());

    let mut duplicate = preparation_spec()?;
    duplicate
        .jobs
        .get_mut(ReleaseRole::RegistryPublishOidc.job_id())
        .expect("registry")
        .needs = vec![
        ReleaseRole::PackageAnonymous.job_id().to_owned(),
        ReleaseRole::PreflightForge.job_id().to_owned(),
        ReleaseRole::PreflightForge.job_id().to_owned(),
    ];
    assert!(invalid(duplicate.validate()).is_some());
    Ok(())
}

#[test]
fn graph_rejects_unknown_self_and_backward_edges() -> Result<(), RenderError> {
    let mut unknown = spec()?;
    unknown
        .jobs
        .get_mut(ReleaseRole::PreflightForge.job_id())
        .expect("preflight")
        .needs
        .push("ghost".to_owned());
    assert!(invalid(unknown.validate()).is_some_and(|text| text.starts_with("unknown_need:")));

    let mut looping = spec()?;
    looping
        .jobs
        .get_mut(ReleaseRole::PreflightForge.job_id())
        .expect("preflight")
        .needs = vec![ReleaseRole::PreflightForge.job_id().to_owned()];
    assert!(invalid(looping.validate()).is_some_and(|text| text.starts_with("self_need:")));

    let mut backward = spec()?;
    backward
        .jobs
        .get_mut(ReleaseRole::PreflightForge.job_id())
        .expect("preflight")
        .needs = vec![ReleaseRole::ForgePublish.job_id().to_owned()];
    assert!(invalid(backward.validate()).is_some_and(|text| text.starts_with("backward_need:")));
    Ok(())
}

#[test]
fn conditions_are_exact_for_every_role() -> Result<(), RenderError> {
    let mut publisher = spec()?;
    publisher
        .jobs
        .get_mut(ReleaseRole::ForgePublish.job_id())
        .expect("forge")
        .condition = Some("true".to_owned());
    assert!(
        invalid(publisher.validate())
            .is_some_and(|text| text.starts_with("release_role_condition:"))
    );

    let mut preparation = preparation_spec()?;
    preparation
        .jobs
        .get_mut(ReleaseRole::PreparationForge.job_id())
        .expect("preparation forge")
        .condition = None;
    assert!(
        invalid(preparation.validate())
            .is_some_and(|text| text.starts_with("release_role_condition:"))
    );

    let mut reconcile = spec()?;
    reconcile
        .jobs
        .get_mut(ReleaseRole::Reconcile.job_id())
        .expect("reconcile")
        .condition = Some("success()".to_owned());
    assert!(
        invalid(reconcile.validate())
            .is_some_and(|text| text.starts_with("release_role_condition:"))
    );

    let mut anonymous = spec()?;
    anonymous
        .jobs
        .get_mut(ReleaseRole::PackageAnonymous.job_id())
        .expect("package")
        .condition = Some("true".to_owned());
    assert!(
        invalid(anonymous.validate())
            .is_some_and(|text| text.starts_with("release_role_condition:"))
    );
    Ok(())
}

#[test]
fn environments_are_exact_for_publishers_preparation_and_validation() -> Result<(), RenderError> {
    let mut publisher = spec()?;
    publisher
        .jobs
        .get_mut(ReleaseRole::RegistryPublishOidc.job_id())
        .expect("registry")
        .environment = Some("wrong".to_owned());
    assert!(
        invalid(publisher.validate())
            .is_some_and(|text| text.starts_with("publish_environment_mismatch:"))
    );

    let mut preparation = preparation_spec()?;
    preparation
        .jobs
        .get_mut(ReleaseRole::PreparationForge.job_id())
        .expect("preparation forge")
        .environment = Some(BOOTSTRAP_ENV.to_owned());
    assert!(
        invalid(preparation.validate())
            .is_some_and(|text| text.starts_with("publish_environment_mismatch:"))
    );

    for role in [
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        ReleaseRole::Reconcile,
    ] {
        let mut workflow = spec()?;
        workflow
            .jobs
            .get_mut(role.job_id())
            .expect("validation job")
            .environment = Some(PUBLISH_ENV.to_owned());
        assert!(invalid(workflow.validate()).is_some());
    }
    Ok(())
}

#[test]
fn bootstrap_publisher_requires_bootstrap_environment() -> Result<(), RenderError> {
    let mut workflow = bootstrap_spec()?;
    workflow
        .jobs
        .get_mut(ReleaseRole::RegistryPublishBootstrap.job_id())
        .expect("registry")
        .environment = Some(PUBLISH_ENV.to_owned());
    assert!(
        invalid(workflow.validate())
            .is_some_and(|text| text.starts_with("publish_environment_mismatch:"))
    );
    Ok(())
}
