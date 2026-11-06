// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryOwnership, CrateRegistryPresence, CrateRegistryRead, CrateReleaseAction,
    CrateReleaseAdmissionReceipt, CrateReleaseIntent, CrateReleasePlannedPackage,
    GitHubAdmissionRead, GitHubAdmissionRequest, GitHubCandidate, OrchestratorError,
    effective_release_intent, inspect_crate_registry, internal, read_github_tag_evidence,
    verify_github_evidence, verify_release_ownership, verify_request_intent, workspace_inventory,
};

pub(super) fn observe_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
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
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_request_intent(intent, request)?;
    let admitted = verify_github_evidence(request, github)?;
    observe_package_state(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
        &admitted,
    )
}

pub(super) fn observe_package_state<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source_context: crate::CrateSource<'_>,
    admitted: &GitHubCandidate,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let source = source_context.root;
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_request_intent(intent, request)?;
    if admitted.request != *request {
        return Err(internal("crate_release_candidate_mismatch"));
    }
    crate::crate_release_github::verify_immutable_tag_rules(&intent.repository, github)?;
    let effective_intent = effective_release_intent(intent, request);
    let inventory = workspace_inventory(
        source,
        &request.source_sha,
        &effective_intent,
        registry,
        source_context.git,
        source_context.execution,
    )?;
    let target = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let mut packages = Vec::with_capacity(inventory.len());
    for package in inventory {
        let tag = format!("{}-v{}", package.name, package.version);
        let tag_sha = read_github_tag_evidence(&intent.repository, &tag, github)?;
        let (owners, presence) = inspect_crate_registry(registry, &package.name, &package.version)?;
        verify_release_ownership(intent, request, github, &package, &owners)?;
        let action = match (presence, tag_sha) {
            (CrateRegistryPresence::Absent, None) => CrateReleaseAction::Publish,
            (CrateRegistryPresence::Absent, Some(_)) => {
                return Err(internal("crate_release_tag_collision"));
            }
            (CrateRegistryPresence::Present, tag_sha) => {
                let archive = crate::CrateSourceExecution::package(
                    source_context.execution,
                    &crate::CratePackageSource::new(
                        source,
                        &effective_intent,
                        &package,
                        target.path(),
                        &request.source_sha,
                        false,
                        source_context.git,
                    ),
                )?;
                let state = crate::crate_release_registry::reconcile_crate_registry(
                    registry,
                    &package.name,
                    &package.version,
                    match &owners {
                        CrateRegistryOwnership::Existing { owners } => owners,
                        CrateRegistryOwnership::NewCrate => {
                            return Err(internal("crate_release_new_crate_inconsistent"));
                        }
                    },
                    &archive,
                )?;
                package_action_for_tag(
                    github,
                    &intent.repository,
                    &tag,
                    &package.version,
                    &request.source_sha,
                    tag_sha,
                    state,
                )?
            }
        };
        packages.push(CrateReleasePlannedPackage {
            source: package,
            tag,
            owners,
            action,
        });
    }
    Ok(CrateReleaseAdmissionReceipt {
        schema: 1,
        intent: intent.clone(),
        github: admitted.clone(),
        packages,
        semver_evidence: std::collections::BTreeMap::new(),
    })
}

pub(super) fn package_action_for_tag<G: GitHubAdmissionRead>(
    github: &G,
    repository: &str,
    tag: &str,
    version: &str,
    source_sha: &str,
    tag_sha: Option<String>,
    state: crate::crate_release_registry::CrateRegistryReconciliation,
) -> Result<CrateReleaseAction, OrchestratorError> {
    match tag_sha {
        None => Ok(CrateReleaseAction::RepairTag { registry: state }),
        Some(sha) if sha == source_sha => {
            match crate::crate_release_github::read_github_release_evidence(
                repository, tag, version, github,
            )? {
                None => Ok(CrateReleaseAction::RepairRelease { registry: state }),
                Some(release) => Ok(CrateReleaseAction::Reconciled {
                    registry: state,
                    release,
                }),
            }
        }
        Some(_) => Err(internal("crate_release_tag_mismatch")),
    }
}
