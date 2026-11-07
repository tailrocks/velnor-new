// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "crate_release_plz.rs"]
mod plz;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) use plz::invoke_supervised_upload;
pub(crate) use plz::invoke_upload;
#[cfg(target_os = "linux")]
pub(crate) use plz::single_package_config;
#[cfg(test)]
pub(crate) fn fixture_upload_config(
    intent: &CrateReleaseIntent,
    package: &str,
) -> Result<String, OrchestratorError> {
    plz::single_package_config(intent, package)
}
#[path = "crate_release_observation.rs"]
mod observation;
pub(crate) use observation::{
    CrateReleaseObservation, FrozenCompletion, ValidatedCompletion, coordinate_release,
    derive_reconciled_candidate, observe_prior_recovery, record_verified_mutation,
    repair_verified_metadata, verify_and_record_upload_intent,
};
#[cfg(test)]
pub(crate) use observation::{RecoveryState, UploadAttempt, reconciled_completion_packages};
pub(crate) use observation::{VerifiedMutation, verify_observed_packages};

mod inventory;
pub(crate) mod preflight;
mod reconcile;
mod state;
use crate::crate_release_github::{
    GitHubAdmissionRead, GitHubAdmissionRequest, GitHubCandidate, authorize_github_candidate,
    read_github_tag_evidence, verify_github_evidence,
};
use crate::crate_release_registry::{
    CrateRegistryOwner, CrateRegistryOwnership, CrateRegistryPresence, CrateRegistryRead,
    CrateRegistryReconciliation, inspect_crate_registry,
};
use crate::release::{crate_release_workspace::CrateSourceExecution, release_identity};
use crate::{
    CrateReleaseSourcePackage, OrchestratorError, crate_release_workspace_plan_with_registry,
    internal::internal,
};
pub(crate) use inventory::package_unprivileged;
use inventory::require_reviewer_owner;
use inventory::{wait_for_release, wait_for_tag, workspace_inventory};
pub(crate) use reconcile::same_observed_state;
pub use reconcile::{
    CrateReleaseCompletedPackage, reconcile_crate_release, validate_reconciliation_receipt,
    verify_crate_release_admission, wait_for_reconciliation_visibility,
};
use serde::{Deserialize, Serialize};
pub(crate) use state::reprepare_crate_release;
use state::{observe_admitted_crate_release, observe_crate_release, observe_package_state};
use std::path::Path;
use velnor_actions_contract::config::CrateReleaseIntent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleasePlannedPackage {
    pub source: CrateReleaseSourcePackage,
    pub tag: String,
    pub owners: CrateRegistryOwnership,
    pub action: CrateReleaseAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CrateReleaseAction {
    Publish,
    RepairTag {
        registry: CrateRegistryReconciliation,
    },
    RepairRelease {
        registry: CrateRegistryReconciliation,
    },
    Reconciled {
        registry: CrateRegistryReconciliation,
        release: crate::crate_release_github::GitHubReleaseEvidence,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseAdmissionReceipt {
    pub schema: u32,
    pub intent: CrateReleaseIntent,
    pub github: GitHubCandidate,
    pub packages: Vec<CrateReleasePlannedPackage>,
    pub semver_evidence:
        std::collections::BTreeMap<String, crate::release::crate_release_semver::SemverEvidence>,
}

pub fn prepare_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source_context: crate::CrateSource<'_>,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let crate::CrateSource {
        root: source,
        git,
        execution: source_execution,
    } = source_context;
    let observed = observe_crate_release(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    state::validate_preparation(observed)
}

#[derive(Debug)]
pub(crate) struct VerifiedReleasePackage {
    intent_sha256: String,
    repository: String,
    workflow_sha: String,
    source_sha: String,
    run_id: u64,
    run_attempt: u32,
    package: String,
    version: String,
}

pub(super) struct PackageVerification<'a, G, R> {
    pub receipt: &'a CrateReleaseAdmissionReceipt,
    pub intent: &'a CrateReleaseIntent,
    pub request: &'a GitHubAdmissionRequest,
    pub github: &'a G,
    pub registry: &'a R,
    pub source: &'a Path,
    pub git: &'a crate::crate_release_git::ReleaseGit,
    pub package: &'a str,
    pub source_execution: &'a dyn CrateSourceExecution,
}

pub(crate) fn effective_release_intent(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
) -> CrateReleaseIntent {
    let mut effective = intent.clone();
    if request.event == "push" {
        effective.recovery = None;
    }
    effective
}

fn verify_release_ownership<R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    registry: &R,
    package: &CrateReleaseSourcePackage,
    owners: &CrateRegistryOwnership,
) -> Result<(), OrchestratorError> {
    match owners {
        CrateRegistryOwnership::Existing { owners } => {
            require_reviewer_owner(registry, owners, intent.trusted_reviewer_id)?;
        }
        CrateRegistryOwnership::NewCrate => {
            if intent.authentication
                != velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken
                || request.event != "workflow_dispatch"
                || intent
                    .new_crates
                    .as_ref()
                    .and_then(|creation| creation.packages.get(&package.name))
                    != Some(&package.version)
            {
                return Err(internal("crate_release_new_crate_not_approved"));
            }
        }
    }
    Ok(())
}

fn verify_request_intent(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
) -> Result<(), OrchestratorError> {
    let recovery = if request.event == "workflow_dispatch" {
        intent.exact_source_sha()
    } else {
        None
    };
    if (intent.exact_source_sha().is_some() && request.event != "workflow_dispatch")
        || !release_identity::same_repository(&request.repository, &intent.repository)
        || request.default_branch != intent.default_branch
        || request.environment != intent.environment
        || request.workflow_path != ".github/workflows/release.yml"
        || request.required_workflow_path != intent.required_workflow_path
        || request.trusted_reviewer_id != intent.trusted_reviewer_id
        || request.recovery_sha.as_deref() != recovery
    {
        return Err(internal("crate_release_intent_request_mismatch"));
    }
    Ok(())
}

pub trait CrateReleaseRepairWrite {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError>;
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
        version: &str,
    ) -> Result<(), OrchestratorError>;
}

pub fn repair_crate_release<
    G: GitHubAdmissionRead + CrateReleaseRepairWrite,
    R: CrateRegistryRead,
>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    let recovery = intent
        .recovery
        .as_ref()
        .ok_or_else(|| internal("crate_release_repair_requires_recovery"))?;
    if request.event != "workflow_dispatch"
        || recovery.source_sha != request.source_sha
        || receipt.packages.len() != 1
        || receipt.packages[0].source.name != recovery.package
        || receipt.packages[0].source.version != recovery.version
    {
        return Err(internal("crate_release_repair_scope"));
    }
    if receipt.schema != 1 || &receipt.intent != intent {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let observe = || {
        observe_admitted_crate_release(
            intent,
            request,
            github,
            registry,
            source_context,
            &receipt.github,
        )
    };
    if !same_observed_state(&observe()?, receipt) {
        return Err(internal("crate_release_repair_state_changed"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let package = &receipt.packages[0];
    match &package.action {
        CrateReleaseAction::RepairTag { registry: expected } => {
            if let Err(error) =
                github.create_tag(&request.repository, &package.tag, &request.source_sha)
            {
                observe()?;
                return Err(error);
            }
            let observed = observe()?;
            authorize_github_candidate(request, &receipt.github, github)?;
            if observed.github != receipt.github
                || observed.packages.len() != 1
                || observed.packages[0].source != package.source
                || observed.packages[0].owners != package.owners
                || observed.packages[0].tag != package.tag
                || !matches!(&observed.packages[0].action, CrateReleaseAction::RepairRelease { registry } if registry == expected)
            {
                return Err(internal("crate_release_repair_state_changed"));
            }
        }
        CrateReleaseAction::RepairRelease { .. } => {}
        _ => return Err(internal("crate_release_repair_action_invalid")),
    }
    if let Err(error) = github.create_release(
        &request.repository,
        &package.tag,
        &request.source_sha,
        &package.source.version,
    ) {
        observe()?;
        return Err(error);
    }
    let observed = observe()?;
    authorize_github_candidate(request, &receipt.github, github)?;
    let (CrateReleaseAction::RepairTag { registry: expected }
    | CrateReleaseAction::RepairRelease { registry: expected }) = &package.action
    else {
        return Err(internal("crate_release_repair_action_invalid"));
    };
    if observed.github != receipt.github
        || observed.packages.len() != 1
        || observed.packages[0].source != package.source
        || observed.packages[0].owners != package.owners
        || observed.packages[0].tag != package.tag
        || !matches!(&observed.packages[0].action, CrateReleaseAction::Reconciled { registry, .. } if registry == expected)
    {
        return Err(internal("crate_release_repair_state_changed"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/crate_release_controller_public.rs"]
mod tests;
