// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "crate_release_plz.rs"]
mod plz;
pub(crate) use plz::invoke_upload;
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
    CrateReleaseObservation, coordinate_release, derive_reconciled_candidate,
    observe_prior_recovery, reconciled_completion_packages, record_verified_mutation,
    repair_verified_metadata, verify_and_record_upload_intent,
};
#[cfg(test)]
pub(crate) use observation::{RecoveryState, UploadAttempt};
pub(crate) use observation::{VerifiedMutation, verify_observed_packages};

use crate::crate_release_github::{
    GitHubAdmissionRead, GitHubAdmissionRequest, GitHubCandidate, authorize_github_candidate,
    read_github_tag_evidence, verify_github_evidence,
};
use crate::crate_release_registry::{
    CrateRegistryOwner, CrateRegistryOwnership, CrateRegistryPresence, CrateRegistryRead,
    CrateRegistryReconciliation, inspect_crate_registry,
};
use crate::{
    CrateReleaseSourcePackage, OrchestratorError, crate_release_workspace_plan_with_registry,
    internal::internal,
};
use serde::{Deserialize, Serialize};
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
        std::collections::BTreeMap<String, crate::crate_release_semver::SemverEvidence>,
}

pub fn prepare_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let observed = observe_crate_release(intent, request, github, registry, source)?;
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
    pub package: &'a str,
}

pub(crate) fn verify_release_package<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    package: &str,
) -> Result<VerifiedReleasePackage, OrchestratorError> {
    let input = PackageVerification {
        receipt,
        intent,
        request,
        github,
        registry,
        source,
        package,
    };
    verify_release_package_with(&input, |observed| {
        observation::verify_observed_packages(observed, receipt)
    })
}

pub(super) fn verify_release_package_with<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &PackageVerification<'_, G, R>,
    validate: impl Fn(&CrateReleaseAdmissionReceipt) -> Result<(), OrchestratorError>,
) -> Result<VerifiedReleasePackage, OrchestratorError> {
    let PackageVerification {
        receipt,
        intent,
        request,
        github,
        registry,
        source,
        package,
    } = *input;
    if receipt.schema != 1 || receipt.intent != *intent || receipt.github.request != *request {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let observed = observe_crate_release(intent, request, github, registry, source)?;
    validate(&observed)?;
    let index = observed
        .packages
        .iter()
        .position(|p| p.source.name == package)
        .ok_or_else(|| internal("crate_release_package_not_selected"))?;
    if observed.packages[..index]
        .iter()
        .any(|p| !matches!(p.action, CrateReleaseAction::Reconciled { .. }))
        || observed.packages[index].action != CrateReleaseAction::Publish
    {
        return Err(internal("crate_release_package_not_ready"));
    }
    let target = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let effective_intent = effective_release_intent(intent, request);
    package_source(
        source,
        &effective_intent,
        &observed.packages[index].source,
        target.path(),
        &request.source_sha,
        true,
    )?;
    if observe_crate_release(intent, request, github, registry, source)? != observed {
        return Err(internal("crate_release_package_state_changed"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let intent_bytes = velnor_actions_contract::canonical_json_bytes(intent)
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    Ok(VerifiedReleasePackage {
        intent_sha256: crate::generator::sha256_hex(&intent_bytes),
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        package: observed.packages[index].source.name.clone(),
        version: observed.packages[index].source.version.clone(),
    })
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

fn observe_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_request_intent(intent, request)?;
    let admitted = verify_github_evidence(request, github)?;
    observe_package_state(intent, request, github, registry, source, &admitted)
}

fn observe_package_state<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    admitted: &GitHubCandidate,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_request_intent(intent, request)?;
    if admitted.request != *request {
        return Err(internal("crate_release_candidate_mismatch"));
    }
    crate::crate_release_github::verify_immutable_tag_rules(&intent.repository, github)?;
    let effective_intent = effective_release_intent(intent, request);
    let inventory = workspace_inventory(source, &request.source_sha, &effective_intent, registry)?;
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
                let archive = package_source(
                    source,
                    &effective_intent,
                    &package,
                    target.path(),
                    &request.source_sha,
                    false,
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
                match tag_sha {
                    None => CrateReleaseAction::RepairTag { registry: state },
                    Some(sha) if sha == request.source_sha => {
                        match crate::crate_release_github::read_github_release_evidence(
                            &intent.repository,
                            &tag,
                            github,
                        )? {
                            None => CrateReleaseAction::RepairRelease { registry: state },
                            Some(release) => CrateReleaseAction::Reconciled {
                                registry: state,
                                release,
                            },
                        }
                    }
                    Some(_) => return Err(internal("crate_release_tag_mismatch")),
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

fn verify_release_ownership<G: GitHubAdmissionRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    package: &CrateReleaseSourcePackage,
    owners: &CrateRegistryOwnership,
) -> Result<(), OrchestratorError> {
    match owners {
        CrateRegistryOwnership::Existing { owners } => {
            require_reviewer_owner(github, owners, intent.trusted_reviewer_id)?;
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
        intent
            .recovery
            .as_ref()
            .map(|item| item.source_sha.as_str())
            .or_else(|| {
                intent
                    .new_crates
                    .as_ref()
                    .map(|item| item.source_sha.as_str())
            })
    } else {
        None
    };
    if (intent.new_crates.is_some() && request.event != "workflow_dispatch")
        || request.repository != intent.repository
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

pub fn validate_reconciliation_receipt<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
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
    let inventory = workspace_inventory(source, &request.source_sha, &effective_intent, registry)?;
    if inventory.len() != receipt.packages.len() || inventory.is_empty() {
        return Err(invalid());
    }
    let mut all_visible = true;
    for (package, planned) in inventory.iter().zip(&receipt.packages) {
        let tag = format!("{}-v{}", package.name, package.version);
        if planned.source != *package
            || planned.tag != tag
            || planned.action != CrateReleaseAction::Publish
        {
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
        if planned.owners != owners {
            return Err(invalid());
        }
        all_visible &= observed_tag.is_some() && presence == CrateRegistryPresence::Present;
    }
    Ok(all_visible)
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
) -> Result<(), OrchestratorError> {
    if receipt.schema != 1 || &receipt.intent != intent {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let fresh = prepare_crate_release(intent, request, github, registry, source)?;
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
) -> Result<Vec<CrateReleaseCompletedPackage>, OrchestratorError> {
    if receipt.schema != 1 || &receipt.intent != intent {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    verify_request_intent(intent, request)?;
    authorize_github_candidate(request, &receipt.github, github)?;
    crate::crate_release_github::verify_immutable_tag_rules(&intent.repository, github)?;
    let effective_intent = effective_release_intent(intent, request);
    let inventory = workspace_inventory(source, &request.source_sha, &effective_intent, registry)?;
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
        let CrateRegistryOwnership::Existing { owners } = &package.owners else {
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

fn workspace_inventory<R: CrateRegistryRead>(
    source: &Path,
    source_sha: &str,
    intent: &CrateReleaseIntent,
    registry: &R,
) -> Result<Vec<CrateReleaseSourcePackage>, OrchestratorError> {
    let workspace =
        crate_release_workspace_plan_with_registry(source, source_sha, intent, registry)?;
    let plan = workspace.plan;
    plan.graph
        .order
        .iter()
        .map(|name| {
            let package = plan
                .selection
                .packages
                .iter()
                .find(|package| &package.name == name)
                .ok_or_else(|| internal("crate_release_workspace_plan_invalid"))?;
            Ok(CrateReleaseSourcePackage {
                name: package.name.clone(),
                version: package.version.clone(),
                manifest: package.manifest.clone(),
            })
        })
        .collect()
}

fn package_source(
    source: &Path,
    intent: &CrateReleaseIntent,
    package: &CrateReleaseSourcePackage,
    target: &Path,
    expected_sha: &str,
    verify: bool,
) -> Result<Vec<u8>, OrchestratorError> {
    use std::ffi::{OsStr, OsString};
    use velnor_actions_mise::{PinnedTool, PinnedToolExec};
    crate::verify_crate_release_source(source, expected_sha)?;
    crate::crate_release_source::verify_toolchain_source_separation(source)?;
    let mut args = vec![
        OsString::from("package"),
        OsString::from("--locked"),
        OsString::from("--manifest-path"),
        source.join(&intent.manifest_path).into_os_string(),
        OsString::from("--package"),
        OsString::from(&package.name),
        OsString::from("--target-dir"),
        target.as_os_str().to_owned(),
    ];
    let features = intent
        .publish_features
        .get(&package.name)
        .ok_or_else(|| internal("crate_release_package_features_missing"))?;
    if !features.is_empty() {
        args.push(OsString::from("--features"));
        args.push(OsString::from(features.join(",")));
    }
    if !verify {
        args.push(OsString::from("--no-verify"));
    }
    let sterile = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let cargo_home = sterile.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).map_err(|_| internal("crate_release_package_temp"))?;
    #[cfg(test)]
    tests::configure_test_registry(&cargo_home)?;
    let catalog = crate::crate_release_source::source_tool_catalog(&intent.rust_version)
        .map_err(|_| internal("crate_release_toolchain_invalid"))?;
    let mut isolated_args = vec![
        OsString::from("-i"),
        OsString::from(format!("HOME={}", sterile.path().display())),
        OsString::from(format!("CARGO_HOME={}", cargo_home.display())),
    ];
    isolated_args.extend(crate::crate_release_source::source_compiler_env(
        &intent.rust_version,
    )?);
    isolated_args.push(
        crate::crate_release_source::source_cargo_path(&intent.rust_version, sterile.path())?
            .into_os_string(),
    );
    isolated_args.extend(args);
    let exec = PinnedToolExec::new(vec![PinnedTool::Rust], OsStr::new("env"), isolated_args)
        .map_err(|_| internal("crate_release_package_command"))?;
    let result = exec
        .command(&catalog)
        .map_err(|_| internal("crate_release_package_command"))?
        .with_cwd(if verify {
            source.to_path_buf()
        } else {
            sterile.path().to_path_buf()
        })
        .run()
        .map_err(|_| internal("crate_release_package_failed"))?;
    if !result.success {
        return Err(internal("crate_release_package_failed"));
    }
    crate::verify_crate_release_source(source, expected_sha)?;
    let relative = format!("package/{}-{}.crate", package.name, package.version);
    match crate::safe_read::read_repo_bytes(target, &relative, 64 * 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(bytes),
        crate::safe_read::RepoBytes::Absent => Err(internal("crate_release_package_missing")),
    }
}

fn require_reviewer_owner<G: GitHubAdmissionRead>(
    github: &G,
    owners: &[CrateRegistryOwner],
    reviewer_id: u64,
) -> Result<(), OrchestratorError> {
    for owner in owners {
        if owner.kind != "user"
            || !velnor_actions_contract::config::is_valid_github_owner(&owner.login)
        {
            continue;
        }
        let identity = github.get(&format!("users/{}", owner.login))?;
        if identity["id"].as_u64() == Some(reviewer_id)
            && identity["type"].as_str() == Some("User")
            && identity["login"]
                .as_str()
                .is_some_and(|login| login.eq_ignore_ascii_case(&owner.login))
        {
            return Ok(());
        }
    }
    Err(internal("crate_release_reviewer_not_owner"))
}

fn wait_for_tag<G: GitHubAdmissionRead>(
    github: &G,
    repository: &str,
    tag: &str,
    expected: &str,
) -> Result<String, OrchestratorError> {
    for attempt in 0..60 {
        match read_github_tag_evidence(repository, tag, github)? {
            Some(sha) if sha == expected => return Ok(sha),
            Some(_) => return Err(internal("crate_release_tag_mismatch")),
            None if attempt < 59 => std::thread::sleep(std::time::Duration::from_secs(2)),
            None => return Err(internal("crate_release_tag_missing")),
        }
    }
    Err(internal("crate_release_tag_missing"))
}

fn wait_for_release<G: GitHubAdmissionRead>(
    github: &G,
    repository: &str,
    tag: &str,
) -> Result<crate::crate_release_github::GitHubReleaseEvidence, OrchestratorError> {
    for attempt in 0..60 {
        match crate::crate_release_github::read_github_release_evidence(repository, tag, github)? {
            Some(url) => return Ok(url),
            None if attempt < 59 => std::thread::sleep(std::time::Duration::from_secs(2)),
            None => return Err(internal("crate_release_github_release_missing")),
        }
    }
    Err(internal("crate_release_github_release_missing"))
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
    source: &Path,
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
    if !same_observed_state(
        &observe_crate_release(intent, request, github, registry, source)?,
        receipt,
    ) {
        return Err(internal("crate_release_repair_state_changed"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let package = &receipt.packages[0];
    match &package.action {
        CrateReleaseAction::RepairTag { registry: expected } => {
            if let Err(error) =
                github.create_tag(&intent.repository, &package.tag, &request.source_sha)
            {
                observe_crate_release(intent, request, github, registry, source)?;
                return Err(error);
            }
            let observed = observe_crate_release(intent, request, github, registry, source)?;
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
    if let Err(error) = github.create_release(&intent.repository, &package.tag, &request.source_sha)
    {
        observe_crate_release(intent, request, github, registry, source)?;
        return Err(error);
    }
    let observed = observe_crate_release(intent, request, github, registry, source)?;
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
