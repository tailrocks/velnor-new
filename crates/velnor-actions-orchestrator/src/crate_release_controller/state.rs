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

pub(super) fn observe_admitted_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source_context: crate::CrateSource<'_>,
    candidate: &GitHubCandidate,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let admitted = crate::crate_release_github::verify_recorded_github_evidence(candidate, github)?;
    observe_package_state(intent, request, github, registry, source_context, &admitted)
}

pub(crate) fn reprepare_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    github: &G,
    registry: &R,
    source_context: crate::CrateSource<'_>,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let observed = observe_admitted_crate_release(
        &receipt.intent,
        &receipt.github.request,
        github,
        registry,
        source_context,
        &receipt.github,
    )?;
    validate_preparation(observed)
}

pub(super) fn validate_preparation(
    observed: CrateReleaseAdmissionReceipt,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let intent = &observed.intent;
    if intent.journal_predecessor.is_none()
        && observed.packages.iter().any(|package| {
            intent
                .new_crates
                .as_ref()
                .is_some_and(|creation| creation.packages.contains_key(&package.source.name))
                && matches!(package.owners, CrateRegistryOwnership::Existing { .. })
        })
    {
        return Err(internal("crate_release_new_crate_already_exists"));
    }
    Ok(observed)
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
    verify_package_authority(intent, request, admitted, github)?;
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
        let tag_sha = read_github_tag_evidence(&request.repository, &tag, github)?;
        let (owners, presence) = inspect_crate_registry(registry, &package.name, &package.version)?;
        verify_release_ownership(intent, request, registry, &package, &owners)?;
        let action = match (presence, tag_sha) {
            (CrateRegistryPresence::Absent, None) => CrateReleaseAction::Publish,
            (CrateRegistryPresence::Absent, Some(_)) => {
                return Err(internal("crate_release_tag_collision"));
            }
            (CrateRegistryPresence::Present, tag_sha) => {
                let archive = super::package_source::package_source(
                    source_context,
                    &effective_intent,
                    &package,
                    target.path(),
                    &request.source_sha,
                )?;
                let owners = match &owners {
                    CrateRegistryOwnership::Existing { owners } => owners,
                    CrateRegistryOwnership::NewCrate => {
                        return Err(internal("crate_release_new_crate_inconsistent"));
                    }
                };
                if tag_sha
                    .as_deref()
                    .is_some_and(|sha| sha != request.source_sha)
                {
                    historical_package_action(
                        github, registry, request, &package, owners, &archive,
                    )?
                } else {
                    let state = crate::crate_release_registry::reconcile_crate_registry(
                        registry,
                        &package.name,
                        &package.version,
                        owners,
                        &archive,
                    )?;
                    package_action_for_tag(
                        github,
                        &request.repository,
                        &tag,
                        &package.version,
                        &request.source_sha,
                        tag_sha,
                        state,
                    )?
                }
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HistoricalGitHubEvidence {
    historical_sha: String,
    current_sha: String,
    repository: String,
    package: String,
    version: String,
    release: crate::crate_release_github::GitHubReleaseEvidence,
}

impl HistoricalGitHubEvidence {
    pub(crate) fn matches_package(&self, repository: &str, name: &str, version: &str) -> bool {
        self.package == name && self.version == version && self.repository == repository
    }
    pub(crate) fn current_sha(&self) -> &str {
        &self.current_sha
    }
    pub(crate) fn historical_sha(&self) -> &str {
        &self.historical_sha
    }
    pub(crate) fn release(&self) -> &crate::crate_release_github::GitHubReleaseEvidence {
        &self.release
    }
    pub(crate) fn authenticate<G: GitHubAdmissionRead>(
        api: &G,
        repository: &str,
        package: &str,
        tag: &str,
        version: &str,
        current_sha: &str,
    ) -> Result<Self, OrchestratorError> {
        let invalid = || internal("crate_release_historical_evidence_invalid");
        if tag != format!("{package}-v{version}") {
            return Err(invalid());
        }
        crate::crate_release_github::verify_immutable_tag_rules(repository, api)?;
        let historical_sha = read_github_tag_evidence(repository, tag, api)?.ok_or_else(invalid)?;
        if historical_sha == current_sha {
            return Err(invalid());
        }
        let comparison = api.get(&format!(
            "repos/{repository}/compare/{historical_sha}...{current_sha}"
        ))?;
        if comparison["status"].as_str() != Some("ahead")
            || comparison["merge_base_commit"]["sha"].as_str() != Some(historical_sha.as_str())
            || comparison["base_commit"]["sha"].as_str() != Some(historical_sha.as_str())
        {
            return Err(invalid());
        }
        let release = crate::crate_release_github::read_github_release_evidence(
            repository, tag, version, api,
        )?
        .ok_or_else(invalid)?;
        if read_github_tag_evidence(repository, tag, api)?.as_deref() != Some(&historical_sha) {
            return Err(invalid());
        }
        Ok(Self {
            historical_sha,
            current_sha: current_sha.to_owned(),
            repository: repository.to_owned(),
            package: package.to_owned(),
            version: version.to_owned(),
            release,
        })
    }
}

fn historical_package_action<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    github: &G,
    registry: &R,
    request: &GitHubAdmissionRequest,
    package: &crate::crate_release_controller::CrateReleaseSourcePackage,
    owners: &[crate::crate_release_registry::CrateRegistryOwner],
    archive: &[u8],
) -> Result<CrateReleaseAction, OrchestratorError> {
    let tag = format!("{}-v{}", package.name, package.version);
    let observe = || {
        HistoricalGitHubEvidence::authenticate(
            github,
            &request.repository,
            &package.name,
            &tag,
            &package.version,
            &request.source_sha,
        )
    };
    let evidence = observe()?;
    let registry = crate::crate_release_registry::reconcile_historical_crate_registry(
        registry,
        &request.repository,
        &package.name,
        &package.version,
        owners,
        archive,
        &evidence,
    )?;
    if evidence != observe()? {
        return Err(internal("crate_release_historical_evidence_changed"));
    }
    Ok(CrateReleaseAction::Historical {
        registry,
        source_sha: evidence.historical_sha,
        release: evidence.release,
    })
}

fn verify_package_authority<G: GitHubAdmissionRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    admitted: &GitHubCandidate,
    github: &G,
) -> Result<(), OrchestratorError> {
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_request_intent(intent, request)?;
    if admitted.request != *request {
        return Err(internal("crate_release_candidate_mismatch"));
    }
    crate::crate_release_github::verify_immutable_tag_rules(&request.repository, github)
}
