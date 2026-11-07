// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(test, unix))]
#[path = "crate_release_semver_test_registry.rs"]
pub(crate) mod test_registry;

#[path = "crate_release_semver/checker.rs"]
mod checker;
#[path = "crate_release_semver/evidence.rs"]
mod evidence;
#[path = "crate_release_semver/metadata.rs"]
mod metadata;
pub(crate) use metadata::{matches_revalidated_preflight, verify_observed_evidence};

pub(crate) use checker::run_check;
use checker::verify_checker;
pub(crate) use checker::verify_one;

use std::{cmp::Ordering, ffi::OsString, io::Write as _, path::Path, time::Duration};

use crate::{
    OrchestratorError,
    crate_registry_inventory::{CrateInventoryError, inspect_crate_versions},
    crate_release_controller::CrateReleaseAdmissionReceipt,
    crate_release_registry::CrateRegistryRead,
    internal::internal,
    release::crate_release_source::{source_cargo_path, source_compiler_env, source_tool_catalog},
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SemverEvidence {
    Checked {
        baseline: String,
        baseline_checksum: String,
        checker_sha256: String,
    },
    NoBaseline {
        checker_sha256: String,
    },
    Deferred {
        predecessors: Vec<String>,
    },
    PriorNoBaselinePending {
        checker_sha256: String,
        target_checksum: String,
    },
    MetadataOnly {
        package: String,
        version: String,
        source_sha: String,
        intent_sha256: String,
        target_checksum: String,
    },
}

pub(crate) struct VerifiedSemver {
    package: String,
    version: String,
    source_sha: String,
    intent_sha256: String,
    evidence: SemverEvidence,
}

impl VerifiedSemver {
    pub(crate) fn into_bound_evidence(
        self,
        package: &str,
        version: &str,
        source_sha: &str,
        intent_sha256: &str,
    ) -> Result<SemverEvidence, OrchestratorError> {
        if self.package != package
            || self.version != version
            || self.source_sha != source_sha
            || self.intent_sha256 != intent_sha256
            || !self.evidence.is_final()
        {
            return Err(invalid());
        }
        if let SemverEvidence::Checked { baseline, .. } = &self.evidence {
            let baseline = semver::Version::parse(baseline).map_err(|_| invalid())?;
            let target = semver::Version::parse(version).map_err(|_| invalid())?;
            if baseline.cmp_precedence(&target) != Ordering::Less {
                return Err(invalid());
            }
        }
        Ok(self.evidence)
    }
}

fn invalid() -> OrchestratorError {
    internal("crate_release_semver_unverified")
}

#[derive(Debug, PartialEq, Eq)]
struct Baseline {
    version: String,
    id: u64,
}

#[derive(serde::Deserialize)]
struct BaselineReply {
    version: BaselineVersion,
}

#[derive(serde::Deserialize)]
struct BaselineVersion {
    id: u64,
    #[serde(rename = "crate")]
    package: String,
    num: String,
    #[serde(rename = "yanked")]
    _yanked: bool,
    checksum: String,
}

fn baseline<R: CrateRegistryRead>(
    registry: &R,
    package: &str,
    version: &str,
) -> Result<Option<Baseline>, OrchestratorError> {
    let target = semver::Version::parse(version).map_err(|_| invalid())?;
    let inventory = match inspect_crate_versions(registry, package) {
        Ok(inventory) => inventory,
        Err(CrateInventoryError::NotFound) => return Ok(None),
        Err(_) => return Err(invalid()),
    };
    if inventory.versions.is_empty() {
        return Err(invalid());
    }
    let mut earlier = Vec::new();
    for (value, entry) in inventory.versions {
        let parsed = semver::Version::parse(&value).map_err(|_| invalid())?;
        if parsed.cmp_precedence(&target) == Ordering::Less {
            earlier.push((parsed, value, entry.id, entry.yanked));
        }
    }
    earlier.sort_by(|left, right| left.0.cmp_precedence(&right.0));
    if earlier
        .windows(2)
        .any(|pair| pair[0].0.cmp_precedence(&pair[1].0) == Ordering::Equal)
    {
        return Err(invalid());
    }
    let selected = earlier
        .iter()
        .rposition(|entry| entry.0.pre.is_empty() && !entry.3)
        .or_else(|| earlier.iter().rposition(|entry| !entry.3))
        .or_else(|| earlier.len().checked_sub(1));
    selected
        .map(|index| {
            let (_, version, id, _) = earlier.swap_remove(index);
            Some(Baseline { version, id })
        })
        .ok_or_else(invalid)
}

pub(crate) fn verify_preflight<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    runner_temp: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<std::collections::BTreeMap<String, SemverEvidence>, OrchestratorError> {
    let executable = receipt
        .packages
        .iter()
        .any(|package| {
            matches!(
                package.action,
                crate::crate_release_controller::CrateReleaseAction::Publish
            )
        })
        .then(|| verify_checker(receipt, runner_temp))
        .transpose()?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    let effective = crate::crate_release_controller::effective_release_intent(
        &receipt.intent,
        &receipt.github.request,
    );
    let workspace =
        crate::release::crate_release_workspace::crate_release_workspace_plan_with_registry(
            source,
            &receipt.github.request.source_sha,
            &effective,
            registry,
            git,
            source_execution,
        )?;
    let mut evidence = std::collections::BTreeMap::new();
    for package in &receipt.packages {
        if !matches!(
            package.action,
            crate::crate_release_controller::CrateReleaseAction::Publish
        ) {
            evidence.insert(
                package.source.name.clone(),
                metadata::verify(source, receipt, registry, package, git, source_execution)?,
            );
            continue;
        }
        let blocked = unpublished_dependencies(receipt, package, &workspace);
        if !blocked.is_empty() {
            eprintln!(
                "SemVer check deferred until selected prerequisite is published: {}",
                package.source.name
            );
            evidence.insert(
                package.source.name.clone(),
                SemverEvidence::Deferred {
                    predecessors: blocked,
                },
            );
            continue;
        }
        let item = if let Some(pending) = prior_no_baseline_pending(receipt, registry, package)? {
            pending
        } else {
            verify_one(
                source,
                receipt,
                registry,
                package,
                executable.as_deref().ok_or_else(invalid)?,
                git,
                source_execution,
            )?
        };
        evidence.insert(package.source.name.clone(), item);
    }
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    Ok(evidence)
}

fn unpublished_dependencies(
    receipt: &CrateReleaseAdmissionReceipt,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
    workspace: &crate::release::crate_release_workspace::CrateReleaseWorkspacePlan,
) -> Vec<String> {
    let mut blocked: Vec<String> = workspace
        .plan
        .graph
        .edges
        .iter()
        .filter(|edge| edge.from == package.source.name)
        .filter(|edge| {
            receipt.packages.iter().any(|other| {
                other.source.name == edge.to
                    && matches!(
                        other.action,
                        crate::crate_release_controller::CrateReleaseAction::Publish
                    )
            })
        })
        .map(|edge| edge.to.clone())
        .collect();
    blocked.sort();
    blocked.dedup();
    blocked
}

pub(crate) fn prior_no_baseline_pending<R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
) -> Result<Option<SemverEvidence>, OrchestratorError> {
    use crate::crate_release_controller::CrateReleaseAction;
    let Some(creation) = &receipt.intent.new_crates else {
        return Ok(None);
    };
    if receipt.intent.journal_predecessor.is_none()
        || receipt.intent.authentication
            != velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken
        || receipt.github.request.event != "workflow_dispatch"
        || creation.source_sha != receipt.github.request.source_sha
        || creation.packages.get(&package.source.name) != Some(&package.source.version)
    {
        return Ok(None);
    }
    let published = match &package.action {
        CrateReleaseAction::RepairTag { registry }
        | CrateReleaseAction::RepairRelease { registry }
        | CrateReleaseAction::Reconciled { registry, .. } => registry,
        CrateReleaseAction::Publish => return Ok(None),
    };
    let inventory =
        inspect_crate_versions(registry, &package.source.name).map_err(|_| invalid())?;
    if inventory.versions.len() != 1
        || inventory
            .versions
            .get(&package.source.version)
            .is_none_or(|v| v.yanked)
        || published.checksum != published.downloaded_checksum
    {
        return Err(invalid());
    }
    let checker =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?;
    Ok(Some(SemverEvidence::PriorNoBaselinePending {
        checker_sha256: checker.executable_sha256.into(),
        target_checksum: published.checksum.clone(),
    }))
}

pub(crate) fn verify_before_upload<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    runner_temp: &Path,
    name: &str,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<VerifiedSemver, OrchestratorError> {
    let executable = verify_checker(receipt, runner_temp)?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    let package = receipt
        .packages
        .iter()
        .find(|package| package.source.name == name)
        .ok_or_else(invalid)?;
    let evidence = verify_one(
        source,
        receipt,
        registry,
        package,
        &executable,
        git,
        source_execution,
    )?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    let intent =
        velnor_actions_contract::canonical_json_bytes(&receipt.intent).map_err(|_| invalid())?;
    Ok(VerifiedSemver {
        package: package.source.name.clone(),
        version: package.source.version.clone(),
        source_sha: receipt.github.request.source_sha.clone(),
        intent_sha256: crate::planner::generator::sha256_hex(&intent),
        evidence,
    })
}

#[cfg(test)]
#[path = "crate_release_semver/tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn fixture_verified_semver(
    receipt: &CrateReleaseAdmissionReceipt,
    name: &str,
) -> Result<VerifiedSemver, OrchestratorError> {
    let package = receipt
        .packages
        .iter()
        .find(|p| p.source.name == name)
        .ok_or_else(invalid)?;
    let intent =
        velnor_actions_contract::canonical_json_bytes(&receipt.intent).map_err(|_| invalid())?;
    Ok(VerifiedSemver {
        package: name.to_owned(),
        version: package.source.version.clone(),
        source_sha: receipt.github.request.source_sha.clone(),
        intent_sha256: crate::planner::generator::sha256_hex(&intent),
        evidence: SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-26.04",
            )
            .ok_or_else(invalid)?
            .executable_sha256
            .into(),
        },
    })
}

#[cfg(test)]
#[path = "crate_release_semver/evidence_tests.rs"]
mod evidence_tests;
