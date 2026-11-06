// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::types::{ExecutionPublicationParent, PublicationKind};
use super::{
    MAX_PARENT_TEXT,
    validation::{lower_hex, valid_boot_id},
};
use crate::{
    error::HostError,
    journal::execution_authorization::{
        ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim,
    },
    journal::{JobFactsPublicationClaim, VerifiedMetadataInitialization},
};

impl ExecutionPublicationParent {
    pub(crate) fn challenge(
        claim: &ExecutionChallengePublicationClaim,
        metadata: &VerifiedMetadataInitialization,
        guest_boot_id: &str,
    ) -> Result<Self, HostError> {
        let parent = Self {
            kind: PublicationKind::Challenge,
            operation_id: claim.operation_id(),
            reservation_id: claim.reservation_id(),
            connection_id: claim.connection_id().to_owned(),
            scale_set_id: claim.scale_set_id(),
            witness_request_id: Some(claim.witness_request_id()),
            actual_request_id: None,
            runner_id: claim.runner_id(),
            runner_name: claim.runner_name().to_owned(),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: claim.dind_container_id().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            metadata_volume_name: metadata.metadata_volume_name().to_owned(),
            metadata_volume_id: metadata.metadata_volume_id().to_owned(),
            metadata_layout_version: metadata.layout_version(),
            metadata_layout_digest: metadata.layout_digest().to_owned(),
            guest_boot_id: guest_boot_id.to_owned(),
            payload_sha256: claim.challenge_digest().to_owned(),
            claim_digest: claim.challenge_digest().to_owned(),
        };
        parent.validate(metadata)?;
        Ok(parent)
    }

    pub(crate) fn decision(
        claim: &ExecutionDecisionDeliveryClaim,
        metadata: &VerifiedMetadataInitialization,
        guest_boot_id: &str,
    ) -> Result<Self, HostError> {
        let parent = Self {
            kind: PublicationKind::Decision,
            operation_id: claim.operation_id(),
            reservation_id: claim.reservation_id(),
            connection_id: claim.connection_id().to_owned(),
            scale_set_id: claim.scale_set_id(),
            witness_request_id: Some(claim.witness_request_id()),
            actual_request_id: Some(claim.actual_request_id()),
            runner_id: claim.runner_id(),
            runner_name: claim.runner_name().to_owned(),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: claim.dind_container_id().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            metadata_volume_name: metadata.metadata_volume_name().to_owned(),
            metadata_volume_id: metadata.metadata_volume_id().to_owned(),
            metadata_layout_version: metadata.layout_version(),
            metadata_layout_digest: metadata.layout_digest().to_owned(),
            guest_boot_id: guest_boot_id.to_owned(),
            payload_sha256: claim.decision_digest().to_owned(),
            claim_digest: claim.decision_digest().to_owned(),
        };
        parent.validate(metadata)?;
        Ok(parent)
    }

    pub(crate) fn job_facts(
        claim: &JobFactsPublicationClaim,
        metadata: &VerifiedMetadataInitialization,
        guest_boot_id: &str,
        payload_sha256: &str,
    ) -> Result<Self, HostError> {
        let runner = claim.runner();
        let parent = Self {
            kind: PublicationKind::JobFacts,
            operation_id: claim.operation_id(),
            reservation_id: claim.reservation_id(),
            connection_id: claim.connection_id().to_owned(),
            scale_set_id: runner.runner_scale_set_id,
            witness_request_id: None,
            actual_request_id: None,
            runner_id: runner.id,
            runner_name: runner.name.clone(),
            runner_container_id: claim.runner_container_id().to_owned(),
            dind_container_id: claim.dind_container_id().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            metadata_volume_name: metadata.metadata_volume_name().to_owned(),
            metadata_volume_id: metadata.metadata_volume_id().to_owned(),
            metadata_layout_version: metadata.layout_version(),
            metadata_layout_digest: metadata.layout_digest().to_owned(),
            guest_boot_id: guest_boot_id.to_owned(),
            payload_sha256: payload_sha256.to_owned(),
            claim_digest: payload_sha256.to_owned(),
        };
        parent.validate(metadata)?;
        Ok(parent)
    }

    pub(super) fn validate(
        &self,
        metadata: &VerifiedMetadataInitialization,
    ) -> Result<(), HostError> {
        if self.operation_id <= 0
            || self.reservation_id != metadata.reservation_id()
            || self.operation_id != metadata.operation_id()
            || self.connection_id != metadata.connection_id()
            || self.engine_id != metadata.engine_id()
            || self.docker_root_digest != metadata.docker_root_digest()
            || self.metadata_volume_name != metadata.metadata_volume_name()
            || self.metadata_volume_id != metadata.metadata_volume_id()
            || self.metadata_layout_version != 2
            || self.metadata_layout_digest != metadata.layout_digest()
            || self.scale_set_id <= 0
            || self.witness_request_id.is_some_and(|value| value <= 0)
            || self.actual_request_id.is_some_and(|value| value <= 0)
            || self.runner_id <= 0
            || self.runner_name.is_empty()
            || self.runner_name.len() > 128
            || self.runner_container_id.len() < 12
            || self.runner_container_id.len() > 128
            || self.dind_container_id.len() < 12
            || self.dind_container_id.len() > 128
            || self.runner_container_id == self.dind_container_id
            || self.engine_id.is_empty()
            || self.engine_id.len() > MAX_PARENT_TEXT
            || self.metadata_volume_name.is_empty()
            || self.metadata_volume_name.len() > 255
            || self.metadata_volume_id.is_empty()
            || self.metadata_volume_id.len() > 255
            || !lower_hex(&self.docker_root_digest, 64)
            || !lower_hex(&self.metadata_layout_digest, 64)
            || !valid_boot_id(&self.guest_boot_id)
            || !lower_hex(&self.payload_sha256, 64)
            || self.payload_sha256 != self.claim_digest
            || !lower_hex(&self.claim_digest, 64)
            || self.parent_kind_valid().is_err()
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    fn parent_kind_valid(&self) -> Result<(), HostError> {
        match (self.kind, self.actual_request_id, self.witness_request_id) {
            (PublicationKind::Challenge, None, Some(_))
            | (PublicationKind::Decision, Some(_), Some(_))
            | (PublicationKind::JobFacts, None, None) => Ok(()),
            _ => Err(HostError::Identity),
        }
    }

    pub(crate) fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(crate) fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }

    pub(crate) fn parent_kind(&self) -> PublicationKind {
        self.kind
    }

    pub(crate) fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn metadata_layout_version(&self) -> i64 {
        self.metadata_layout_version
    }

    pub(crate) fn metadata_layout_digest(&self) -> &str {
        &self.metadata_layout_digest
    }
}
