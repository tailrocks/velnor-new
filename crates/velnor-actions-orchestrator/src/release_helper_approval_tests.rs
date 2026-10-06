//! Generator authority rejects coherent record and source substitutions.

use std::collections::BTreeMap;

use super::*;
use velnor_actions_contract::{
    HelperInvocation, SourceBoundHelper,
    config::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig},
};
use velnor_actions_rust::release_select::{
    ReleaseRequest, ReleaseScope, ReleaseSelection, select_release_set,
};
use velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn fixture(action: impl FnOnce(&JobInputs<'_>) -> TestResult) -> TestResult {
    fixture_mode(ReleaseAuthentication::TrustedPublishing, true, action)
}

fn fixture_mode(
    authentication: ReleaseAuthentication,
    release_pr: bool,
    action: impl FnOnce(&JobInputs<'_>) -> TestResult,
) -> TestResult {
    let selection = selection_fixture()?;
    let packages: BTreeMap<_, _> = selection
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    let bootstrap =
        (authentication == ReleaseAuthentication::BootstrapToken).then(|| BootstrapRelease {
            package: "demo".to_owned(),
            version: "0.1.0".to_owned(),
            source_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        });
    let release = RustReleaseConfig {
        enabled: true,
        packages: vec!["demo".to_owned()],
        expected_owners: BTreeMap::from([("demo".to_owned(), vec!["user:1".to_owned()])]),
        authentication,
        release_pr,
        bootstrap,
        ..RustReleaseConfig::default()
    };
    let catalog = ToolCatalog::pinned();
    let plan = BootstrapPlan {
        plan_id: "release-0123456789ab".to_owned(),
        repository: "acme/widgets".to_owned(),
        source_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        registry: "crates-io".to_owned(),
        packages: packages.clone(),
        version: release
            .bootstrap
            .as_ref()
            .map(|record| record.version.clone()),
    };
    let policy = reconcile::approved_policy(&release, &plan, &catalog)?;
    let bootstrap_tools = ReleaseBootstrapApproval {
        checkout_uses: velnor_actions_actionlint::actions::PinnedActionRef::checkout().uses_value(),
        mise: crate::test_mise::setup("2026.1.0", &"a".repeat(64)),
    };
    action(&JobInputs {
        release: &release,
        selection: &selection,
        gate: String::new(),
        sha: &plan.source_sha,
        repository: &plan.repository,
        branch: "main",
        packages: &packages,
        reconciliation: &policy,
        actual_registry: &plan.registry,
        label: "ubuntu-24.04",
        bootstrap_tools: &bootstrap_tools,
        catalog: &catalog,
    })
}

fn rebound<'a>(
    inputs: &'a JobInputs<'a>,
    label: &'a str,
    reconciliation: &'a velnor_actions_workflow_renderer::release_spec::ReleaseReconcilePolicy,
) -> JobInputs<'a> {
    JobInputs {
        release: inputs.release,
        selection: inputs.selection,
        gate: inputs.gate.clone(),
        sha: inputs.sha,
        repository: inputs.repository,
        branch: inputs.branch,
        packages: inputs.packages,
        reconciliation,
        actual_registry: inputs.actual_registry,
        label,
        bootstrap_tools: inputs.bootstrap_tools,
        catalog: inputs.catalog,
    }
}

fn selection_fixture() -> Result<ReleaseSelection, Box<dyn std::error::Error>> {
    let root = std::path::Path::new("/velnor-release-qualification-fixture");
    let metadata = serde_json::json!({
        "version": 1,
        "workspace_root": root,
        "workspace_members": ["demo"],
        "packages": [{
            "id": "demo", "name": "demo", "version": "0.1.0",
            "manifest_path": root.join("Cargo.toml"),
            "publish": null, "features": {}, "dependencies": [],
            "targets": [{ "kind": ["lib"], "name": "demo", "test": true,
                "doctest": true, "required_features": [] }],
        }],
    })
    .to_string();
    Ok(select_release_set(&ReleaseRequest {
        metadata_json: &metadata,
        repo_root: root,
        manifest_hint: "Cargo.toml",
        scope: &ReleaseScope::Packages(vec!["demo".to_owned()]),
        enabled: true,
        affected: &std::collections::BTreeSet::new(),
        supported_registries: &[],
    })?)
}

#[test]
fn assembled_registry_receives_generation_seal_in_every_mode() -> TestResult {
    for authentication in [
        ReleaseAuthentication::TrustedPublishing,
        ReleaseAuthentication::BootstrapToken,
    ] {
        for release_pr in [false, true] {
            fixture_mode(authentication, release_pr, |inputs| {
                let records = super::super::release_steps::helper_registry(inputs)?;
                let sources = super::super::release_support_sources::complete_support_sources(
                    inputs, VERSION,
                )?;
                let approved = approve(inputs, records.clone(), sources.clone(), VERSION)?;
                assert_eq!(approved.into_parts(), (records, sources));
                Ok(())
            })?;
        }
    }
    Ok(())
}

#[test]
fn coherent_helper_and_registry_substitution_is_rejected() -> TestResult {
    fixture(|inputs| {
        let mut records = expected_registry(inputs)?;
        let original = &records[4];
        let source = velnor_actions_contract::generated_source(VERSION, "exit 0\n")?;
        let descriptor = original.invocation().descriptor();
        let replacement = SourceBoundHelper::compiled(
            descriptor.operation(),
            descriptor.path(),
            &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
        )?;
        let invocation = HelperInvocation::compiled(
            replacement,
            original.invocation().args().to_vec(),
            original.invocation().installed_selectors().to_vec(),
        )?;
        let mut forged = CompiledSourceHelper::compiled(invocation, source)?
            .with_environment(original.environment().clone());
        if let Some(recipe) = original.execution_recipe() {
            forged = forged.with_execution_recipe(recipe.clone())?;
        }
        if original.github_output() {
            forged = forged.with_github_output()?;
        }
        // The graph step and registry agree perfectly; only its compiled owner disagrees.
        let step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
            "Coherent substituted helper",
            &forged,
            forged.environment().clone(),
        )?;
        assert!(matches!(
            step.kind,
            velnor_actions_contract::StepKind::SourceBoundHelper { .. }
        ));
        records[4] = forged;
        let sources =
            super::super::release_support_sources::complete_support_sources(inputs, VERSION)?;
        let error = approve(inputs, records, sources, VERSION)
            .err()
            .ok_or("forgery admitted")?;
        assert!(
            error
                .to_string()
                .contains("release_helper_registry_authority")
        );
        Ok(())
    })
}

#[test]
fn changed_actual_host_cannot_reuse_a_qualified_registry() -> TestResult {
    fixture(|inputs| {
        let records = expected_registry(inputs)?;
        let sources =
            super::super::release_support_sources::complete_support_sources(inputs, VERSION)?;
        let changed = rebound(inputs, "ubuntu-24.04-arm", inputs.reconciliation);
        let error = approve(&changed, records, sources, VERSION)
            .err()
            .ok_or("host drift admitted")?;
        let problem = error.to_string();
        assert!(
            problem.contains("release_helper_registry_authority")
                || problem.contains("source_snapshot_tools:")
        );
        Ok(())
    })
}

#[test]
fn changed_policy_cannot_requalify_matching_records() -> TestResult {
    fixture(|inputs| {
        let mut policy = inputs.reconciliation.clone();
        policy
            .tags
            .insert("demo".to_owned(), "demo-v9.9.9".to_owned());
        let changed = rebound(inputs, inputs.label, &policy);
        let records = expected_registry(&changed)?;
        let sources =
            super::super::release_support_sources::complete_support_sources(inputs, VERSION)?;
        let error = approve(&changed, records, sources, VERSION)
            .err()
            .ok_or("policy drift admitted")?;
        assert!(
            error
                .to_string()
                .contains("release_helper_policy_authority")
        );
        Ok(())
    })
}

#[test]
fn complete_sources_cannot_be_substituted_independently() -> TestResult {
    fixture(|inputs| {
        let records = expected_registry(inputs)?;
        let mut sources =
            super::super::release_support_sources::complete_support_sources(inputs, VERSION)?;
        let path = sources[0].path().to_owned();
        sources[0] = CompiledSupportSource::compiled(&path, "pass\n", VERSION)?;
        let error = approve(inputs, records, sources, VERSION)
            .err()
            .ok_or("source substitution admitted")?;
        assert!(
            error
                .to_string()
                .contains("release_helper_source_authority")
        );
        Ok(())
    })
}
