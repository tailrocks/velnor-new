// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryOwnership, CrateRegistryPresence, CrateRegistryRead, CrateReleaseAction,
    CrateReleaseAdmissionReceipt, CrateReleaseIntent, CrateReleaseSourcePackage, Deserialize,
    GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, Path, Serialize,
    authorize_github_candidate, effective_release_intent, inspect_crate_registry, internal,
    package_source, prepare_crate_release, read_github_tag_evidence, require_reviewer_owner,
    verify_github_evidence, verify_observed_packages, verify_request_intent, wait_for_release,
    wait_for_tag, workspace_inventory,
};

pub fn validate_reconciliation_receipt<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<bool, OrchestratorError> {
    let invalid = || internal("crate_release_reconciliation_receipt_invalid");
    if receipt.schema != 1
        || &receipt.intent != intent
        || receipt.github != verify_github_evidence(request, github)?
    {
        return Err(invalid());
    }
    verify_request_intent(intent, request)?;
    crate::crate_release_github::verify_immutable_tag_rules(&intent.repository, github)?;
    let effective_intent = effective_release_intent(intent, request);
    let inventory = workspace_inventory(
        source,
        &request.source_sha,
        &effective_intent,
        registry,
        git,
    )?;
    if inventory.len() != receipt.packages.len() || inventory.is_empty() {
        return Err(invalid());
    }
    let mut all_visible = true;
    for (package, planned) in inventory.iter().zip(&receipt.packages) {
        let tag = format!("{}-v{}", package.name, package.version);
        if planned.source != *package || planned.tag != tag {
            return Err(invalid());
        }
        let observed_tag = read_github_tag_evidence(&intent.repository, &tag, github)?;
        if observed_tag
            .as_deref()
            .is_some_and(|sha| sha != request.source_sha)
        {
            return Err(invalid());
        }
        let (owners, presence) =
            match inspect_crate_registry(registry, &package.name, &package.version) {
                Err(OrchestratorError::Internal { problem })
                    if problem == "crate_registry_versions_inconsistent" =>
                {
                    return Ok(false);
                }
                result => result?,
            };
        if let CrateRegistryOwnership::Existing { owners } = &owners {
            require_reviewer_owner(github, owners, intent.trusted_reviewer_id)?;
        }
        verify_reconciliation_owners(intent, request, planned, &owners)?;
        all_visible &= observed_tag.is_some() && presence == CrateRegistryPresence::Present;
    }
    Ok(all_visible)
}

fn verify_reconciliation_owners(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    planned: &super::CrateReleasePlannedPackage,
    observed: &CrateRegistryOwnership,
) -> Result<(), OrchestratorError> {
    let approved_creation = matches!(planned.owners, CrateRegistryOwnership::NewCrate)
        && matches!(observed, CrateRegistryOwnership::Existing { .. })
        && planned.action == CrateReleaseAction::Publish
        && request.event == "workflow_dispatch"
        && intent.authentication
            == velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken
        && intent.new_crates.as_ref().is_some_and(|creation| {
            creation.source_sha == request.source_sha
                && creation.packages.get(&planned.source.name) == Some(&planned.source.version)
        });
    if planned.owners != *observed && !approved_creation {
        return Err(internal("crate_release_reconciliation_receipt_invalid"));
    }
    Ok(())
}

pub fn wait_for_reconciliation_visibility(
    attempts: usize,
    mut observe: impl FnMut() -> Result<bool, OrchestratorError>,
    mut wait: impl FnMut(),
) -> Result<(), OrchestratorError> {
    if attempts == 0 {
        return Err(internal("crate_release_visibility_wait_invalid"));
    }
    for attempt in 0..attempts {
        if observe()? {
            return Ok(());
        }
        if attempt + 1 < attempts {
            wait();
        }
    }
    Err(internal("crate_release_visibility_timeout"))
}

pub(crate) fn same_observed_state(
    left: &CrateReleaseAdmissionReceipt,
    right: &CrateReleaseAdmissionReceipt,
) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.semver_evidence.clear();
    right.semver_evidence.clear();
    left == right
}

pub fn verify_crate_release_admission<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    if receipt.schema != 1 || &receipt.intent != intent {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let fresh = prepare_crate_release(intent, request, github, registry, source, git)?;
    verify_observed_packages(&fresh, receipt)?;
    authorize_github_candidate(request, &receipt.github, github)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateReleaseCompletedPackage {
    pub source: CrateReleaseSourcePackage,
    pub tag: String,
    pub tag_commit: String,
    pub release_id: u64,
    pub release_url: String,
    pub source_inventory: std::collections::BTreeMap<String, (u32, String)>,
    pub registry: crate::crate_release_registry::CrateRegistryReconciliation,
}

pub fn reconcile_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<CrateReleaseCompletedPackage>, OrchestratorError> {
    if receipt.schema != 1 || &receipt.intent != intent {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    verify_request_intent(intent, request)?;
    authorize_github_candidate(request, &receipt.github, github)?;
    crate::crate_release_github::verify_immutable_tag_rules(&intent.repository, github)?;
    let effective_intent = effective_release_intent(intent, request);
    let inventory = workspace_inventory(
        source,
        &request.source_sha,
        &effective_intent,
        registry,
        git,
    )?;
    if inventory.len() != receipt.packages.len()
        || inventory
            .iter()
            .zip(&receipt.packages)
            .any(|(actual, planned)| actual != &planned.source)
    {
        return Err(internal("crate_release_inventory_mismatch"));
    }
    let target = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let mut completed = Vec::with_capacity(receipt.packages.len());
    for package in &receipt.packages {
        let (observed, _) =
            inspect_crate_registry(registry, &package.source.name, &package.source.version)?;
        verify_reconciliation_owners(intent, request, package, &observed)?;
        let CrateRegistryOwnership::Existing { owners } = &observed else {
            return Err(internal("crate_release_new_owner_unverified"));
        };
        require_reviewer_owner(github, owners, intent.trusted_reviewer_id)?;
        if package.tag != format!("{}-v{}", package.source.name, package.source.version) {
            return Err(internal("crate_release_tag_mismatch"));
        }
        let tag_sha = wait_for_tag(
            github,
            &intent.repository,
            &package.tag,
            &request.source_sha,
        )?;
        let archive = package_source(
            source,
            intent,
            &package.source,
            target.path(),
            &request.source_sha,
            false,
            git,
        )?;
        crate::crate_release_registry::wait_for_crate_version(
            registry,
            &package.source.name,
            &package.source.version,
            60,
            || std::thread::sleep(std::time::Duration::from_secs(2)),
        )?;
        let source_inventory = crate::crate_release_archive::crate_archive_inventory(
            &archive,
            &package.source.name,
            &package.source.version,
        )?;
        let registry_result = crate::crate_release_registry::reconcile_crate_registry(
            registry,
            &package.source.name,
            &package.source.version,
            owners,
            &archive,
        )?;
        let release = wait_for_release(github, &intent.repository, &package.tag)?;
        completed.push(CrateReleaseCompletedPackage {
            source: package.source.clone(),
            tag: package.tag.clone(),
            tag_commit: tag_sha,
            release_id: release.id,
            release_url: release.url,
            source_inventory,
            registry: registry_result,
        });
    }
    Ok(completed)
}
