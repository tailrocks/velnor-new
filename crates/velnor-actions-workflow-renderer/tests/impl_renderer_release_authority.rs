//! Closed publisher authority and branch binding regression cases.
use std::collections::BTreeMap;

use super::{REPO, ReleaseRole, RenderError, invalid, job, publish_gate_condition, spec};
use velnor_actions_contract::config::ReleaseAuthentication;
use velnor_actions_workflow_renderer::release_spec::{BootstrapPlan, ReleaseReconcilePolicy};

pub(crate) fn reconciliation(plan: &BootstrapPlan) -> ReleaseReconcilePolicy {
    ReleaseReconcilePolicy {
        schema: 1,
        repository: plan.repository.clone(),
        registry: "crates-io".to_owned(),
        source_sha: plan.source_sha.clone(),
        packages: plan.packages.clone(),
        owners: plan
            .packages
            .keys()
            .map(|name| (name.clone(), vec!["user:1".to_owned()]))
            .collect(),
        tags: plan
            .packages
            .iter()
            .map(|(name, version)| (name.clone(), format!("{name}-v{version}")))
            .collect(),
        authentication: if plan.version.is_some() {
            ReleaseAuthentication::BootstrapToken
        } else {
            ReleaseAuthentication::TrustedPublishing
        },
        tools: BTreeMap::from([
            ("generator".to_owned(), "0.1.0".to_owned()),
            ("gh".to_owned(), "2.102.0".to_owned()),
            ("python".to_owned(), "3.14.7".to_owned()),
            ("release-plz".to_owned(), "0.3.169".to_owned()),
            ("rust".to_owned(), "1.98.1".to_owned()),
        ]),
        intent_id: format!("release-{}", plan.source_sha),
    }
}

#[test]
fn publisher_environment_must_match_its_declared_authority() -> Result<(), RenderError> {
    for environment in [Some("other-environment"), None] {
        let mut workflow = spec()?;
        workflow
            .jobs
            .get_mut("release-publish")
            .expect("publish")
            .environment = environment.map(str::to_owned);
        assert!(workflow.validate().is_err());
    }
    Ok(())
}

#[test]
fn branch_authority_rejects_condition_injection() -> Result<(), RenderError> {
    for branch in [
        "main' || true || '",
        "main/{injected}",
        "../main",
        "main\\bad",
        "main:bad",
    ] {
        let mut workflow = spec()?;
        workflow.triggers.push_branches = vec![branch.to_owned()];
        assert!(workflow.validate().is_err(), "branch {branch}");
    }
    let mut workflow = spec()?;
    workflow.triggers.push_branches.push("other".to_owned());
    assert!(workflow.validate().is_err());
    Ok(())
}

#[test]
fn bootstrap_and_oidc_authority_never_coexist() -> Result<(), RenderError> {
    let mut workflow = spec()?;
    let gate = publish_gate_condition(REPO, &workflow.bootstrap, "main");
    workflow.jobs.insert(
        "release-bootstrap".to_owned(),
        job(
            ReleaseRole::RegistryPublishBootstrap,
            &["release-preflight"],
            Some(&gate),
            Some("crates-io-bootstrap"),
        )?,
    );
    assert!(workflow.validate().is_err());
    Ok(())
}

#[test]
fn bootstrap_repository_mismatch_fails_closed() -> Result<(), RenderError> {
    let mut forked = spec()?;
    forked.bootstrap.repository = "mallory/widgets".to_owned();
    assert_eq!(
        invalid(forked.validate()).expect("reject"),
        "bootstrap_repository_mismatch"
    );
    Ok(())
}
