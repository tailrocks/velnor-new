// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CapacityReservation, JournalCompletion, JournalMutation, JournalMutationKind,
    JournalMutationPhase, JournalMutationResult, JournalOperation, JournalOwner, JournalOwnerKind,
    JournalOwnership, JournalPublishedPackage, JournalRecord, JournalStage, MAX_RECORD_BYTES,
    MutationRecord, OrchestratorError, PackageEnvelope, check, invalid,
};
use crate::crate_release_journal::JournalPackage;
use crate::release::crate_release_semver::SemverEvidence;

impl CapacityReservation {
    pub(super) fn check_sizes(&self) -> Result<(), OrchestratorError> {
        let identity = &self.receipt.identity;
        check(&JournalRecord {
            schema: super::super::receipt_schema(&self.receipt).ok_or_else(invalid)?,
            identity: identity.clone(),
            stage: JournalStage::Started,
            receipt: self.receipt.clone(),
            completion: None,
        })?;
        let mut completed = Vec::with_capacity(self.receipt.packages.len());
        let mut aggregate = 0usize;
        for package in &self.receipt.packages {
            let envelope = self.packages.get(&package.name).ok_or_else(invalid)?;
            let owners = worst_owners(&package.owners);
            self.check_upload(package, envelope, &owners)?;
            self.check_release(package)?;
            let published = self.worst_completed(package, envelope, owners);
            let size = velnor_actions_contract::canonical_json_bytes(&published)
                .map_err(|_| invalid())?
                .len();
            aggregate = aggregate
                .checked_add(size)
                .filter(|size| *size <= MAX_RECORD_BYTES)
                .ok_or_else(invalid)?;
            completed.push(published);
        }
        check(&JournalRecord {
            schema: super::super::receipt_schema(&self.receipt).ok_or_else(invalid)?,
            identity: identity.clone(),
            stage: JournalStage::Completed,
            receipt: self.receipt.clone(),
            completion: Some(JournalCompletion {
                packages: completed,
                post_publication_file_hashes: self.receipt.expected_tools.clone(),
            }),
        })
    }

    fn check_upload(
        &self,
        package: &JournalPackage,
        envelope: &PackageEnvelope,
        owners: &[JournalOwner],
    ) -> Result<(), OrchestratorError> {
        let evidence = match self
            .receipt
            .semver_evidence
            .get(&package.name)
            .ok_or_else(invalid)?
        {
            SemverEvidence::MetadataOnly { .. } | SemverEvidence::PriorNoBaselinePending { .. } => {
                return Ok(());
            }
            SemverEvidence::Deferred { .. } => SemverEvidence::Checked {
                baseline: format!("0.0.0-{}", "z".repeat(8 * 1024 * 1024 - 6)),
                baseline_checksum: "f".repeat(64),
                checker_sha256: "f".repeat(64),
            },
            evidence => evidence.clone(),
        };
        let result = JournalMutationResult::Upload {
            registry_checksum: "f".repeat(64),
            downloaded_checksum: "f".repeat(64),
            owners: owners.to_vec(),
            features: envelope.features.clone(),
            files: envelope.files.clone(),
        };
        self.check_mutation(JournalMutation {
            archive_sha256: None,
            sequence: 1000,
            package: package.name.clone(),
            version: package.version.clone(),
            kind: JournalMutationKind::Upload,
            semver_evidence: None,
            phase: JournalMutationPhase::Observed { result },
        })?;
        self.check_mutation(JournalMutation {
            archive_sha256: Some("f".repeat(64)),
            sequence: 1000,
            package: package.name.clone(),
            version: package.version.clone(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(evidence),
            phase: JournalMutationPhase::Intended,
        })
    }

    fn check_release(&self, package: &JournalPackage) -> Result<(), OrchestratorError> {
        let identity = &self.receipt.identity;
        self.check_mutation(JournalMutation {
            archive_sha256: None,
            sequence: 1000,
            package: package.name.clone(),
            version: package.version.clone(),
            kind: JournalMutationKind::CreateRelease,
            semver_evidence: None,
            phase: JournalMutationPhase::Observed {
                result: JournalMutationResult::Release {
                    id: u64::MAX,
                    tag: package.tag.clone(),
                    url: format!(
                        "https://github.com/{}/releases/tag/{}",
                        identity.repository,
                        super::super::schema::percent_encode(&package.tag)
                    ),
                },
            },
        })
    }

    fn check_mutation(&self, mutation: JournalMutation) -> Result<(), OrchestratorError> {
        check(&MutationRecord {
            schema: super::super::receipt_schema(&self.receipt).ok_or_else(invalid)?,
            identity: self.receipt.identity.clone(),
            receipt: self.receipt.clone(),
            mutation,
        })
    }

    fn worst_completed(
        &self,
        package: &JournalPackage,
        envelope: &PackageEnvelope,
        owners: Vec<JournalOwner>,
    ) -> JournalPublishedPackage {
        let identity = &self.receipt.identity;
        let tag = super::super::schema::percent_encode(&package.tag);
        JournalPublishedPackage {
            name: package.name.clone(),
            version: package.version.clone(),
            owners,
            registry_checksum: "f".repeat(64),
            downloaded_checksum: "f".repeat(64),
            tag_target: identity.source_sha.clone(),
            release_id: u64::MAX,
            release_url: format!(
                "https://github.com/{}/releases/tag/{tag}",
                identity.repository
            ),
            files: envelope.files.clone(),
            features: envelope.features.clone(),
            operations: vec![JournalOperation::ObservedPresent; 4],
        }
    }
}

fn worst_owners(ownership: &JournalOwnership) -> Vec<JournalOwner> {
    match ownership {
        JournalOwnership::Existing { owners } => owners.clone(),
        JournalOwnership::NewCrate => (0..256)
            .map(|index| JournalOwner {
                id: u64::MAX - 255 + index,
                login: "z".repeat(256),
                kind: JournalOwnerKind::Team,
            })
            .collect(),
    }
}
