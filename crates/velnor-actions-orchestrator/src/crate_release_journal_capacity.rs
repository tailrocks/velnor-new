// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JournalCompletion, JournalFile, JournalFileKind, JournalMutation, JournalMutationKind,
    JournalMutationPhase, JournalMutationResult, JournalOperation, JournalOwner, JournalOwnerKind,
    JournalOwnership, JournalPublishedPackage, JournalReceipt, JournalRecord, JournalStage,
    MAX_RECORD_BYTES, MutationRecord, OrchestratorError, invalid,
};
#[path = "crate_release_journal_capacity_features.rs"]
mod features;
#[path = "crate_release_journal_capacity_records.rs"]
mod records;
#[path = "crate_release_journal_capacity_source.rs"]
mod source;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

struct PackageEnvelope {
    files: Vec<JournalFile>,
    features: BTreeMap<String, Vec<String>>,
}

pub(crate) struct CapacityReservation {
    receipt: JournalReceipt,
    packages: BTreeMap<String, PackageEnvelope>,
}

impl CapacityReservation {
    pub(crate) fn prepare(
        receipt: &JournalReceipt,
        admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        request: &crate::crate_release_github::GitHubAdmissionRequest,
        source: &Path,
        git: &crate::crate_release_git::ReleaseGit,
    ) -> Result<Self, OrchestratorError> {
        let deadline =
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(600))
                .map_err(|_| invalid())?;
        deadline
            .scope(|| {
                Self::prepare_until(receipt, admission, intent, request, source, git, deadline)
            })
            .map_err(|_| invalid())?
    }

    fn prepare_until(
        receipt: &JournalReceipt,
        admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        request: &crate::crate_release_github::GitHubAdmissionRequest,
        source: &Path,
        git: &crate::crate_release_git::ReleaseGit,
        deadline: velnor_actions_mise::CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        if admission.intent != *intent
            || admission.github.request != *request
            || receipt.identity.repository != request.repository
            || receipt.identity.source_sha != request.source_sha
            || receipt.identity.workflow_sha != request.workflow_sha
            || receipt.identity.run_id != request.run_id
            || receipt.identity.run_attempt != request.run_attempt
            || admission.packages.len() != receipt.packages.len()
        {
            return Err(invalid());
        }
        let intent_bytes =
            velnor_actions_contract::canonical_json_bytes(intent).map_err(|_| invalid())?;
        if receipt.intent_sha256 != crate::planner::generator::sha256_hex(&intent_bytes) {
            return Err(invalid());
        }
        crate::verify_journal_source(source, receipt, git)?;
        crate::release::crate_release_workspace::verify_no_ambient_cargo_config(source)?;
        crate::release::crate_release_source::verify_toolchain_source_separation(source)?;
        let temporary = tempfile::tempdir().map_err(|_| invalid())?;
        std::fs::create_dir(temporary.path().join("cargo-home")).map_err(|_| invalid())?;
        let effective = crate::crate_release_controller::effective_release_intent(intent, request);
        let packages = source::collect_packages(
            receipt,
            admission,
            &effective,
            source,
            git,
            temporary.path(),
            deadline,
        )?;
        let reservation = Self {
            receipt: receipt.clone(),
            packages,
        };
        for package in &admission.packages {
            use crate::crate_release_controller::CrateReleaseAction;
            let registry = match &package.action {
                CrateReleaseAction::Publish => None,
                CrateReleaseAction::RepairTag { registry }
                | CrateReleaseAction::RepairRelease { registry }
                | CrateReleaseAction::Reconciled { registry, .. }
                | CrateReleaseAction::Historical { registry, .. } => Some(registry),
            };
            if let Some(registry) = registry {
                reservation.verify_contents(&package.source.name, &[], &registry.features)?;
            }
        }
        reservation.check_sizes()?;
        crate::verify_journal_source(source, receipt, git)?;
        crate::release::crate_release_workspace::verify_no_ambient_cargo_config(source)?;
        crate::release::crate_release_source::verify_toolchain_source_separation(source)?;
        deadline.remaining().map_err(|_| invalid())?;
        Ok(reservation)
    }

    pub(crate) fn verify_archive(
        &self,
        name: &str,
        version: &str,
        archive: &[u8],
    ) -> Result<(), OrchestratorError> {
        let envelope = self.packages.get(name).ok_or_else(invalid)?;
        let files =
            crate::release::crate_release_archive::crate_archive_inventory(archive, name, version)?;
        let allowed: BTreeSet<_> = envelope
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        if files.keys().any(|path| !allowed.contains(path.as_str())) {
            return Err(invalid());
        }
        let features =
            crate::release::crate_release_archive::crate_archive_features(archive, name, version)?;
        for (name, members) in &features {
            let allowed = envelope.features.get(name).ok_or_else(invalid)?;
            let mut allowed = allowed.iter();
            for member in members {
                if !allowed.by_ref().any(|candidate| candidate == member) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn verify_mutation(
        &self,
        mutation: &JournalMutation,
    ) -> Result<(), OrchestratorError> {
        if let JournalMutationPhase::Observed {
            result:
                JournalMutationResult::Upload {
                    files, features, ..
                },
        } = &mutation.phase
        {
            self.verify_contents(&mutation.package, files, features)?;
        }
        check(&MutationRecord {
            schema: super::receipt_schema(&self.receipt).ok_or_else(invalid)?,
            identity: self.receipt.identity.clone(),
            receipt: self.receipt.clone(),
            mutation: mutation.clone(),
        })
    }

    pub(crate) fn verify_completion(
        &self,
        completion: &JournalCompletion,
    ) -> Result<(), OrchestratorError> {
        for package in &completion.packages {
            self.verify_contents(&package.name, &package.files, &package.features)?;
        }
        check(&JournalRecord {
            schema: super::receipt_schema(&self.receipt).ok_or_else(invalid)?,
            identity: self.receipt.identity.clone(),
            stage: JournalStage::Completed,
            receipt: self.receipt.clone(),
            completion: Some(completion.clone()),
        })
    }

    fn verify_contents(
        &self,
        name: &str,
        files: &[JournalFile],
        features: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), OrchestratorError> {
        let envelope = self.packages.get(name).ok_or_else(invalid)?;
        let allowed: BTreeSet<_> = envelope
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        if files
            .iter()
            .any(|file| !allowed.contains(file.path.as_str()))
        {
            return Err(invalid());
        }
        for (name, members) in features {
            let allowed = envelope.features.get(name).ok_or_else(invalid)?;
            let mut allowed = allowed.iter();
            for member in members {
                if !allowed.by_ref().any(|candidate| candidate == member) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn verify_sequence_capacity(
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    history_len: usize,
    pending: bool,
) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::CrateReleaseAction;
    let required = admission.packages.iter().try_fold(
        history_len
            .checked_add(usize::from(pending))
            .ok_or_else(invalid)?,
        |total, package| {
            let count = match package.action {
                CrateReleaseAction::Publish => 6,
                CrateReleaseAction::RepairTag { .. } => 4,
                CrateReleaseAction::RepairRelease { .. } => 2,
                CrateReleaseAction::Reconciled { .. } | CrateReleaseAction::Historical { .. } => 0,
            };
            total.checked_add(count).ok_or_else(invalid)
        },
    )?;
    if required > 1000 {
        return Err(invalid());
    }
    Ok(())
}

fn check(value: &impl serde::Serialize) -> Result<(), OrchestratorError> {
    let bytes = velnor_actions_contract::canonical_json_bytes(value).map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    Ok(())
}
