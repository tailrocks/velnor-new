//! Generation-only qualification of the complete fixed release helper closure.

use velnor_actions_contract::{CompiledSourceHelper, CompiledSupportSource};
use velnor_actions_mise::{
    ToolCatalog,
    catalog::{delivery_tools, qualification::DistributionHost},
};
use velnor_actions_workflow_renderer::release_spec::{BootstrapPlan, validate_source_sha};

use super::release_admission::{AdmissionIdentity, AdmissionMode, executable};
use super::release_steps::{JobInputs, reconcile};
use crate::OrchestratorError;

/// A seal exists only after the generator reconstructs every source owner record.
/// It has no configuration or serialization constructor.
pub(super) struct QualifiedReleaseHelpers {
    records: Vec<CompiledSourceHelper>,
    sources: Vec<CompiledSupportSource>,
}

impl QualifiedReleaseHelpers {
    /// Consume the qualification seal when freezing the renderer inputs.
    pub(super) fn into_parts(self) -> (Vec<CompiledSourceHelper>, Vec<CompiledSupportSource>) {
        (self.records, self.sources)
    }
}

/// Qualify supplied records against independently reconstructed compiled owners.
pub(super) fn approve(
    inputs: &JobInputs<'_>,
    records: Vec<CompiledSourceHelper>,
    sources: Vec<CompiledSupportSource>,
    version: &str,
) -> Result<QualifiedReleaseHelpers, OrchestratorError> {
    validate_policy(inputs)?;
    if version != env!("CARGO_PKG_VERSION") {
        return Err(rejected("release_helper_generator_version"));
    }
    let expected = expected_registry(inputs)?;
    compare_records(&records, &expected)?;
    let expected_sources =
        super::release_support_sources::complete_support_sources(inputs, version)?;
    if sources != expected_sources {
        return Err(rejected("release_helper_source_authority"));
    }
    Ok(QualifiedReleaseHelpers { records, sources })
}

fn validate_policy(inputs: &JobInputs<'_>) -> Result<(), OrchestratorError> {
    inputs.release.validate(crate::config::CONFIG_REL)?;
    validate_source_sha(inputs.sha)?;
    if !inputs.release.enabled
        || (!inputs.release.publishable_workspace
            && inputs.packages.keys().ne(inputs.release.packages.iter()))
        || inputs.release.bootstrap.as_ref().is_some_and(|record| {
            record.source_sha != inputs.sha
                || inputs.packages.len() != 1
                || inputs.packages.get(&record.package) != Some(&record.version)
        })
    {
        return Err(rejected("release_helper_scope_authority"));
    }
    let catalog = ToolCatalog::pinned();
    if inputs.catalog != &catalog {
        return Err(rejected("release_helper_catalog_authority"));
    }
    let plan = BootstrapPlan {
        plan_id: super::release_identity::plan_id_for_source(inputs.sha),
        repository: inputs.repository.to_owned(),
        source_sha: inputs.sha.to_owned(),
        registry: inputs.actual_registry.to_owned(),
        packages: inputs.packages.clone(),
        version: inputs
            .release
            .bootstrap
            .as_ref()
            .map(|record| record.version.clone()),
    };
    plan.validate()?;
    let expected = reconcile::approved_policy(inputs.release, &plan, &catalog)?;
    if inputs.reconciliation != &expected {
        return Err(rejected("release_helper_policy_authority"));
    }
    Ok(())
}

fn expected_registry(
    inputs: &JobInputs<'_>,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let host = match velnor_actions_contract::tool_target_for_runner_label(inputs.label) {
        Some("x86_64-unknown-linux-gnu") => DistributionHost::LinuxAmd64,
        Some("aarch64-unknown-linux-gnu") => DistributionHost::LinuxArm64,
        _ => return Err(rejected("release_helper_host_authority")),
    };
    let tools = delivery_tools::source_snapshot_tools(host, env!("CARGO_PKG_VERSION")).map_err(
        |error| OrchestratorError::Contract {
            problem: format!("source_snapshot_tools:{error}"),
        },
    )?;
    let mut records: Vec<_> = tools
        .bootstrap_records()
        .iter()
        .chain(tools.preparation_records())
        .cloned()
        .collect();
    for kind in super::release_support_sources::proof_record_kinds(
        inputs.release.authentication,
        inputs.release.release_pr,
    ) {
        records.push(reconcile::proof_record(inputs, kind)?);
    }
    for kind in super::release_support_sources::preparation_record_kinds(inputs.release.release_pr)
    {
        records.push(reconcile::preparation_record(inputs, kind)?);
    }
    let identity = AdmissionIdentity::approved(inputs.repository, inputs.branch, inputs.sha)?;
    records.push(executable(
        AdmissionMode::DefaultBranch,
        &identity,
        env!("CARGO_PKG_VERSION"),
        host,
        velnor_actions_contract::workflow::native_tools::NativeCredentialScope::GithubReadOnly,
    )?);
    Ok(records)
}

fn compare_records(
    records: &[CompiledSourceHelper],
    expected: &[CompiledSourceHelper],
) -> Result<(), OrchestratorError> {
    if records != expected {
        return Err(rejected("release_helper_registry_authority"));
    }
    Ok(())
}

fn rejected(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
#[path = "release_helper_approval_tests.rs"]
mod tests;
