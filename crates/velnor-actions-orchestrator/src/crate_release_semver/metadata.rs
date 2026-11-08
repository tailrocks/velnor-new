// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAdmissionReceipt, OrchestratorError, Path, SemverEvidence,
    invalid,
};

pub(super) fn verify<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
    git: &crate::crate_release_git::ReleaseGit,
    execution: &dyn crate::CrateSourceExecution,
) -> Result<SemverEvidence, OrchestratorError> {
    use crate::crate_release_registry::{CrateRegistryOwnership, CrateRegistryPresence};
    let expected = metadata_registry(&package.action)?;
    let mut named = receipt
        .packages
        .iter()
        .filter(|item| item.source.name == package.source.name);
    if named.next() != Some(package) || named.next().is_some() {
        return Err(invalid());
    }
    let CrateRegistryOwnership::Existing { owners } = &package.owners else {
        return Err(invalid());
    };
    let (observed_owners, presence) = crate::crate_release_registry::inspect_crate_registry(
        registry,
        &package.source.name,
        &package.source.version,
    )?;
    if observed_owners != package.owners || presence != CrateRegistryPresence::Present {
        return Err(invalid());
    }
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    let target = tempfile::tempdir().map_err(|_| invalid())?;
    let intent = crate::crate_release_controller::effective_release_intent(
        &receipt.intent,
        &receipt.github.request,
    );
    let archive = execution.package(&crate::CratePackageSource::new(
        source,
        &intent,
        &package.source,
        target.path(),
        &receipt.github.request.source_sha,
        false,
        git,
    ))?;
    let observed = crate::crate_release_registry::reconcile_crate_registry(
        registry,
        &package.source.name,
        &package.source.version,
        owners,
        &archive,
    )?;
    if observed != *expected {
        return Err(invalid());
    }
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha, git)?;
    let bytes =
        velnor_actions_contract::canonical_json_bytes(&receipt.intent).map_err(|_| invalid())?;
    Ok(SemverEvidence::MetadataOnly {
        package: package.source.name.clone(),
        version: package.source.version.clone(),
        source_sha: receipt.github.request.source_sha.clone(),
        intent_sha256: crate::planner::generator::sha256_hex(&bytes),
        target_checksum: observed.checksum,
    })
}

pub(crate) fn verify_observed_evidence(
    admission: &CrateReleaseAdmissionReceipt,
    observed: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::CrateReleaseAction;
    if admission.intent != observed.intent
        || admission.github != observed.github
        || admission.packages.len() != observed.packages.len()
        || admission.semver_evidence.len() != admission.packages.len()
    {
        return Err(invalid());
    }
    let intent =
        velnor_actions_contract::canonical_json_bytes(&admission.intent).map_err(|_| invalid())?;
    let intent_sha256 = crate::planner::generator::sha256_hex(&intent);
    let mut seen = std::collections::BTreeSet::new();
    for admitted in &admission.packages {
        let name = &admitted.source.name;
        if !seen.insert(name) {
            return Err(invalid());
        }
        let mut matching = observed
            .packages
            .iter()
            .filter(|item| &item.source.name == name);
        let selected = matching.next().ok_or_else(invalid)?;
        if matching.next().is_some()
            || admitted.source != selected.source
            || admitted.owners != selected.owners
        {
            return Err(invalid());
        }
        let evidence = admission.semver_evidence.get(name).ok_or_else(invalid)?;
        if matches!(selected.action, CrateReleaseAction::Publish) {
            if !matches!(admitted.action, CrateReleaseAction::Publish)
                || matches!(
                    evidence,
                    SemverEvidence::MetadataOnly { .. }
                        | SemverEvidence::PriorNoBaselinePending { .. }
                )
            {
                return Err(invalid());
            }
        } else {
            verify_metadata_binding(admission, admitted, selected, evidence, &intent_sha256)?;
        }
    }
    Ok(())
}

fn verify_metadata_binding(
    admission: &CrateReleaseAdmissionReceipt,
    admitted: &crate::crate_release_controller::CrateReleasePlannedPackage,
    selected: &crate::crate_release_controller::CrateReleasePlannedPackage,
    evidence: &SemverEvidence,
    expected_intent: &str,
) -> Result<(), OrchestratorError> {
    let observed = metadata_registry(&selected.action)?;
    let prior = metadata_registry(&admitted.action)?;
    let SemverEvidence::MetadataOnly {
        package,
        version,
        source_sha,
        intent_sha256,
        target_checksum,
    } = evidence
    else {
        return Err(invalid());
    };
    if package != &selected.source.name
        || version != &selected.source.version
        || source_sha != &admission.github.request.source_sha
        || intent_sha256 != expected_intent
        || target_checksum != &observed.checksum
        || prior != observed
        || observed.checksum != observed.downloaded_checksum
        || !matches!(
            selected.owners,
            crate::crate_release_registry::CrateRegistryOwnership::Existing { .. }
        )
    {
        return Err(invalid());
    }
    Ok(())
}

fn metadata_registry(
    action: &crate::crate_release_controller::CrateReleaseAction,
) -> Result<&crate::crate_release_registry::CrateRegistryReconciliation, OrchestratorError> {
    use crate::crate_release_controller::CrateReleaseAction;
    match action {
        CrateReleaseAction::RepairTag { registry }
        | CrateReleaseAction::RepairRelease { registry }
        | CrateReleaseAction::Reconciled { registry, .. } => Ok(registry),
        CrateReleaseAction::Publish => Err(invalid()),
    }
}

pub(crate) fn matches_revalidated_preflight(
    prior: &std::collections::BTreeMap<String, SemverEvidence>,
    fresh: &std::collections::BTreeMap<String, SemverEvidence>,
) -> bool {
    prior.len() == fresh.len()
        && prior.iter().all(|(name, old)| {
            fresh
                .get(name)
                .is_some_and(|new| old.matches_revalidated(new))
        })
}
