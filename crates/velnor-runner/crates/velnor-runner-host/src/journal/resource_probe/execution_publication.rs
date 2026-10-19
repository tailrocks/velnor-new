//! Parent identity and durable child claims for published control files.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::ProbePhase;
use crate::journal::connection::storage::in_transaction;
use crate::{
    error::HostError,
    journal::JobFactsPublicationClaim,
    journal::Journal,
    journal::VerifiedMetadataInitialization,
    journal::execution_authorization::{
        ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim,
    },
    launch::VerifiedExecutionPublicationCleanup,
};

const MAX_PARENT_TEXT: usize = 256;
const MAX_RECOVERY_PROJECTION_BYTES: usize = 32 * 1024;

/// Durable projection facts needed to inspect and remove a publication helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionPublicationRecoveryRecord {
    pub(crate) operation_id: String,
    pub(crate) instance_id: String,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) source_revision: String,
    pub(crate) runtime_image_id: String,
    pub(crate) image_binding_digest: String,
    pub(crate) projection_digest: String,
    pub(crate) context_digest: String,
    pub(crate) recovery_projection_json: String,
    pub(crate) recovery_projection_sha256: String,
    pub(crate) parent_kind: String,
    pub(crate) parent_operation_id: i64,
    pub(crate) reservation_id: i64,
    pub(crate) connection_id: String,
    pub(crate) runner_id: i64,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) metadata_volume_name: String,
    pub(crate) metadata_volume_id: String,
    pub(crate) metadata_layout_version: i64,
    pub(crate) metadata_layout_digest: String,
    pub(crate) guest_boot_id: String,
    pub(crate) payload_sha256: String,
}

/// One publication parent derived from a durable challenge or decision claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionPublicationParent {
    kind: PublicationKind,
    operation_id: i64,
    reservation_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: Option<i64>,
    actual_request_id: Option<i64>,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    engine_id: String,
    docker_root_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    metadata_layout_version: i64,
    metadata_layout_digest: String,
    guest_boot_id: String,
    payload_sha256: String,
    claim_digest: String,
}

/// One fixed file kind in the parent authorization record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationKind {
    /// The challenge file before Runner.Listener receives JIT data.
    Challenge,
    /// The decision file after the hook proves the workflow source.
    Decision,
    /// The cache admission file before JIT delivery.
    JobFacts,
}

impl PublicationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Challenge => "challenge",
            Self::Decision => "decision",
            Self::JobFacts => "job-facts",
        }
    }
}

/// Durable identity for one publication child operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionPublicationContext {
    operation_id: String,
    instance_id: String,
    engine_id: String,
    docker_root_digest: String,
    operation_name: String,
    source_revision: String,
    runtime_image_id: String,
    image_binding_digest: String,
    projection_digest: String,
    recovery_projection_json: String,
    recovery_projection_sha256: String,
    parent: ExecutionPublicationParent,
    context_digest: String,
}

impl ExecutionPublicationParent {
    #[cfg(test)]
    pub(crate) fn test_fixture(
        engine_id: &str,
        docker_root_digest: &str,
        metadata_volume: &str,
        payload_sha256: &str,
    ) -> Self {
        Self {
            kind: PublicationKind::Challenge,
            operation_id: 1,
            reservation_id: 2,
            connection_id: "private-connection".to_owned(),
            scale_set_id: 3,
            witness_request_id: Some(4),
            actual_request_id: None,
            runner_id: 5,
            runner_name: format!("v{}", "1".repeat(32)),
            runner_container_id: "a".repeat(64),
            dind_container_id: "b".repeat(64),
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            metadata_volume_name: metadata_volume.to_owned(),
            metadata_volume_id: metadata_volume.to_owned(),
            metadata_layout_version: 2,
            metadata_layout_digest: "d".repeat(64),
            guest_boot_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
            payload_sha256: payload_sha256.to_owned(),
            claim_digest: payload_sha256.to_owned(),
        }
    }

    /// Bind a new child to one exact claimed challenge and ready metadata volume.
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

    /// Bind a new child to one exact claimed decision and ready metadata volume.
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

    /// Bind a publisher child to one claimed cache admission document.
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

    fn validate(&self, metadata: &VerifiedMetadataInitialization) -> Result<(), HostError> {
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

/// Start identity for one isolated publication container.
pub(crate) struct ExecutionPublicationSeed {
    pub(crate) operation_id: String,
    pub(crate) instance_id: String,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) source_revision: String,
    pub(crate) runtime_image_id: String,
    pub(crate) image_binding_digest: String,
    pub(crate) projection_digest: String,
    pub(crate) recovery_projection_json: String,
    pub(crate) parent: ExecutionPublicationParent,
}

impl ExecutionPublicationSeed {
    pub(crate) fn into_context(
        self,
        metadata: &VerifiedMetadataInitialization,
    ) -> Result<ExecutionPublicationContext, HostError> {
        self.parent.validate(metadata)?;
        let operation_name = format!("velnor-execution-publication-{}", self.operation_id);
        let mut context = ExecutionPublicationContext {
            operation_id: self.operation_id,
            instance_id: self.instance_id,
            engine_id: self.engine_id,
            docker_root_digest: self.docker_root_digest,
            operation_name,
            source_revision: self.source_revision,
            runtime_image_id: self.runtime_image_id,
            image_binding_digest: self.image_binding_digest,
            projection_digest: self.projection_digest,
            recovery_projection_sha256: digest_hex(self.recovery_projection_json.as_bytes()),
            recovery_projection_json: self.recovery_projection_json,
            parent: self.parent,
            context_digest: String::new(),
        };
        context.context_digest = context.compute_digest()?;
        if !context.valid()
            || context.parent.engine_id != context.engine_id
            || context.parent.docker_root_digest != context.docker_root_digest
        {
            return Err(HostError::Identity);
        }
        Ok(context)
    }
}

#[derive(Serialize)]
struct ContextDigest<'a> {
    operation_id: &'a str,
    instance_id: &'a str,
    engine_id: &'a str,
    docker_root_digest: &'a str,
    operation_name: &'a str,
    source_revision: &'a str,
    runtime_image_id: &'a str,
    image_binding_digest: &'a str,
    projection_digest: &'a str,
    recovery_projection_sha256: &'a str,
    parent: ParentDigest<'a>,
}

#[derive(Serialize)]
struct ParentDigest<'a> {
    kind: &'a str,
    operation_id: i64,
    reservation_id: i64,
    connection_id: &'a str,
    scale_set_id: i64,
    witness_request_id: Option<i64>,
    actual_request_id: Option<i64>,
    runner_id: i64,
    runner_name: &'a str,
    runner_container_id: &'a str,
    dind_container_id: &'a str,
    engine_id: &'a str,
    docker_root_digest: &'a str,
    metadata_volume_name: &'a str,
    metadata_volume_id: &'a str,
    metadata_layout_version: i64,
    metadata_layout_digest: &'a str,
    guest_boot_id: &'a str,
    payload_sha256: &'a str,
    claim_digest: &'a str,
}

impl ExecutionPublicationContext {
    fn compute_digest(&self) -> Result<String, HostError> {
        let bytes = serde_json::to_vec(&ContextDigest {
            operation_id: &self.operation_id,
            instance_id: &self.instance_id,
            engine_id: &self.engine_id,
            docker_root_digest: &self.docker_root_digest,
            operation_name: &self.operation_name,
            source_revision: &self.source_revision,
            runtime_image_id: &self.runtime_image_id,
            image_binding_digest: &self.image_binding_digest,
            projection_digest: &self.projection_digest,
            recovery_projection_sha256: &self.recovery_projection_sha256,
            parent: ParentDigest {
                kind: self.parent.kind.as_str(),
                operation_id: self.parent.operation_id,
                reservation_id: self.parent.reservation_id,
                connection_id: &self.parent.connection_id,
                scale_set_id: self.parent.scale_set_id,
                witness_request_id: self.parent.witness_request_id,
                actual_request_id: self.parent.actual_request_id,
                runner_id: self.parent.runner_id,
                runner_name: &self.parent.runner_name,
                runner_container_id: &self.parent.runner_container_id,
                dind_container_id: &self.parent.dind_container_id,
                engine_id: &self.parent.engine_id,
                docker_root_digest: &self.parent.docker_root_digest,
                metadata_volume_name: &self.parent.metadata_volume_name,
                metadata_volume_id: &self.parent.metadata_volume_id,
                metadata_layout_version: self.parent.metadata_layout_version,
                metadata_layout_digest: &self.parent.metadata_layout_digest,
                guest_boot_id: &self.parent.guest_boot_id,
                payload_sha256: &self.parent.payload_sha256,
                claim_digest: &self.parent.claim_digest,
            },
        })
        .map_err(|_| HostError::Journal)?;
        Ok(encode_hex(&Sha256::digest(bytes)))
    }

    pub(crate) fn valid(&self) -> bool {
        lower_hex(&self.operation_id, 32)
            && lower_hex(&self.instance_id, 32)
            && !self.engine_id.is_empty()
            && self.engine_id.len() <= MAX_PARENT_TEXT
            && !self.engine_id.chars().any(char::is_control)
            && lower_hex(&self.docker_root_digest, 64)
            && lower_hex(&self.source_revision, 40)
            && self.runtime_image_id.starts_with("sha256:")
            && lower_hex(self.runtime_image_id.trim_start_matches("sha256:"), 64)
            && lower_hex(&self.image_binding_digest, 64)
            && lower_hex(&self.projection_digest, 64)
            && !self.recovery_projection_json.is_empty()
            && self.recovery_projection_json.len() <= MAX_RECOVERY_PROJECTION_BYTES
            && lower_hex(&self.recovery_projection_sha256, 64)
            && self.recovery_projection_sha256
                == digest_hex(self.recovery_projection_json.as_bytes())
            && self.operation_name == format!("velnor-execution-publication-{}", self.operation_id)
            && self.context_digest == self.compute_digest().unwrap_or_default()
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub(crate) fn runtime_image_id(&self) -> &str {
        &self.runtime_image_id
    }

    pub(crate) fn image_binding_digest(&self) -> &str {
        &self.image_binding_digest
    }

    pub(crate) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(crate) fn projection_digest(&self) -> &str {
        &self.projection_digest
    }

    pub(crate) fn context_digest(&self) -> &str {
        &self.context_digest
    }

    pub(crate) fn parent(&self) -> &ExecutionPublicationParent {
        &self.parent
    }

    pub(crate) fn recovery_projection_json(&self) -> &str {
        &self.recovery_projection_json
    }

    pub(crate) fn recovery_projection_sha256(&self) -> &str {
        &self.recovery_projection_sha256
    }
}

impl Journal {
    /// Load the immutable parent and cleanup facts for one publication helper.
    pub(crate) async fn execution_publication_recovery(
        &self,
        operation_id: &str,
    ) -> Result<Option<ExecutionPublicationRecoveryRecord>, HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
        let mut rows = connection.query(
                "SELECT operation.instance_id, operation.engine_id, operation.docker_root_digest, operation.source_revision, operation.runtime_image_id, operation.image_binding_digest, operation.projection_digest, publication.context_digest, publication.recovery_projection_json, publication.recovery_projection_sha256, publication.parent_kind, publication.parent_operation_id, publication.reservation_id, publication.connection_id, publication.runner_id, publication.runner_container_id, publication.dind_container_id, publication.metadata_volume_name, publication.metadata_volume_id, publication.metadata_layout_version, publication.metadata_layout_digest, publication.guest_boot_id, publication.payload_sha256 FROM resource_probe_operations AS operation JOIN execution_publication_operations AS publication ON publication.operation_id = operation.operation_id WHERE operation.operation_id = ?1 AND operation.helper_role = 'execution-publication' AND operation.operation_name = 'velnor-execution-publication-' || operation.operation_id",
                [operation_id],
            ).await.map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Ok(None);
        };
        let Some(recovery_projection_json) = row
            .get::<Option<String>>(8)
            .map_err(|_| HostError::Journal)?
        else {
            return Ok(None);
        };
        let Some(recovery_projection_sha256) = row
            .get::<Option<String>>(9)
            .map_err(|_| HostError::Journal)?
        else {
            return Ok(None);
        };
        let value = ExecutionPublicationRecoveryRecord {
            operation_id: operation_id.to_owned(),
            instance_id: row.get(0).map_err(|_| HostError::Journal)?,
            engine_id: row.get(1).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
            source_revision: row.get(3).map_err(|_| HostError::Journal)?,
            runtime_image_id: row.get(4).map_err(|_| HostError::Journal)?,
            image_binding_digest: row.get(5).map_err(|_| HostError::Journal)?,
            projection_digest: row.get(6).map_err(|_| HostError::Journal)?,
            context_digest: row.get(7).map_err(|_| HostError::Journal)?,
            recovery_projection_json,
            recovery_projection_sha256,
            parent_kind: row.get(10).map_err(|_| HostError::Journal)?,
            parent_operation_id: row.get(11).map_err(|_| HostError::Journal)?,
            reservation_id: row.get(12).map_err(|_| HostError::Journal)?,
            connection_id: row.get(13).map_err(|_| HostError::Journal)?,
            runner_id: row.get(14).map_err(|_| HostError::Journal)?,
            runner_container_id: row.get(15).map_err(|_| HostError::Journal)?,
            dind_container_id: row.get(16).map_err(|_| HostError::Journal)?,
            metadata_volume_name: row.get(17).map_err(|_| HostError::Journal)?,
            metadata_volume_id: row.get(18).map_err(|_| HostError::Journal)?,
            metadata_layout_version: row.get(19).map_err(|_| HostError::Journal)?,
            metadata_layout_digest: row.get(20).map_err(|_| HostError::Journal)?,
            guest_boot_id: row.get(21).map_err(|_| HostError::Journal)?,
            payload_sha256: row.get(22).map_err(|_| HostError::Journal)?,
        };
        if value.recovery_projection_json.is_empty()
            || value.recovery_projection_json.len() > MAX_RECOVERY_PROJECTION_BYTES
            || !lower_hex(&value.recovery_projection_sha256, 64)
            || value.recovery_projection_sha256
                != digest_hex(value.recovery_projection_json.as_bytes())
            || !lower_hex(&value.context_digest, 64)
        {
            return Err(HostError::Ownership);
        }
        Ok(Some(value))
        }).await
    }

    /// Claim one publisher operation under the shared host lease.
    pub(crate) async fn prepare_execution_publication(
        &self,
        context: ExecutionPublicationContext,
    ) -> Result<(), HostError> {
        if !context.valid()
            || context.parent.engine_id != context.engine_id
            || context.parent.docker_root_digest != context.docker_root_digest
        {
            return Err(HostError::Identity);
        }
        in_transaction(self, async |connection| {
            if crate::journal_schema::instance_id(connection).await? != context.instance_id
                || crate::journal_schema::engine_id(connection).await? != context.engine_id
            {
                return Err(HostError::Identity);
            }
            super::operations::ensure_no_active(connection).await?;
            super::lease::prepare_identity(
                connection,
                &context.operation_id,
                &context.engine_id,
                &context.docker_root_digest,
                &context.operation_name,
            )
            .await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared', 'execution-publication', 'none', NULL, NULL)",
                    turso::params![
                        context.operation_id.as_str(),
                        context.instance_id.as_str(),
                        context.engine_id.as_str(),
                        context.docker_root_digest.as_str(),
                        context.source_revision.as_str(),
                        context.runtime_image_id.as_str(),
                        context.image_binding_digest.as_str(),
                        context.operation_name.as_str(),
                        context.projection_digest.as_str(),
                    ],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let parent = &context.parent;
            connection
                .execute(
                    "INSERT INTO execution_publication_operations (operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest, recovery_projection_json, recovery_projection_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                    turso::params![
                        context.operation_id.as_str(),
                        parent.kind.as_str(),
                        parent.operation_id,
                        parent.reservation_id,
                        parent.connection_id.as_str(),
                        parent.scale_set_id,
                        parent.witness_request_id,
                        parent.actual_request_id,
                        parent.runner_id,
                        parent.runner_name.as_str(),
                        parent.runner_container_id.as_str(),
                        parent.dind_container_id.as_str(),
                        parent.engine_id.as_str(),
                        parent.docker_root_digest.as_str(),
                        parent.metadata_volume_name.as_str(),
                        parent.metadata_volume_id.as_str(),
                        parent.metadata_layout_version,
                        parent.metadata_layout_digest.as_str(),
                        parent.guest_boot_id.as_str(),
                        parent.payload_sha256.as_str(),
                        parent.claim_digest.as_str(),
                        context.projection_digest.as_str(),
                        context.context_digest.as_str(),
                        context.recovery_projection_json.as_str(),
                        context.recovery_projection_sha256.as_str(),
                    ],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Record one legal publication-helper phase transition under its role.
    pub(crate) async fn transition_execution_publication(
        &self,
        operation_id: &str,
        expected: ProbePhase,
        next: ProbePhase,
    ) -> Result<(), HostError> {
        if !expected.allows(next)
            || matches!(next, ProbePhase::Removed | ProbePhase::Aborted)
            || !lower_hex(operation_id, 32)
        {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = ?1 WHERE operation_id = ?2 AND helper_role = 'execution-publication' AND phase = ?3",
                    (next.as_str(), operation_id, expected.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let state = publication_lease_state(next)?;
            super::lease::transition(connection, operation_id, state).await
        })
        .await
    }

    /// Bind one create result to the exact publication operation and lease.
    pub(crate) async fn bind_execution_publication_container(
        &self,
        operation_id: &str,
        container_id: &str,
    ) -> Result<(), HostError> {
        if !lower_hex(operation_id, 32) || !valid_container_id(container_id) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET container_id = ?1, phase = 'container_created' WHERE operation_id = ?2 AND helper_role = 'execution-publication' AND operation_name = 'velnor-execution-publication-' || operation_id AND phase = 'create_requested' AND container_id IS NULL",
                    (container_id, operation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            super::lease::bind_container(connection, operation_id, container_id).await
        })
        .await
    }

    /// Release the shared helper lease after exact publication-container removal.
    pub(crate) async fn retire_execution_publication(
        &self,
        proof: VerifiedExecutionPublicationCleanup,
    ) -> Result<(), HostError> {
        let operation_id = proof.operation_id();
        if proof.phase() != ProbePhase::RemoveRequested
            || !lower_hex(operation_id, 32)
            || proof.operation_name() != format!("velnor-execution-publication-{operation_id}")
            || !valid_container_id(proof.container_id())
        {
            return Err(HostError::Ownership);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'removed' WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND phase = 'remove_requested' AND engine_id = ?2 AND docker_root_digest = ?3 AND operation_name = ?4 AND container_id = ?5",
                    (operation_id, proof.engine_id(), proof.docker_root_digest(), proof.operation_name(), proof.container_id()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Ownership);
            }
            super::lease::release(
                connection,
                operation_id,
                proof.engine_id(),
                proof.docker_root_digest(),
                proof.operation_name(),
                proof.container_id(),
            )
            .await
        })
        .await
    }

    /// Quarantine only the exact publication operation during restart recovery.
    pub(crate) async fn quarantine_execution_publication(
        &self,
        operation_id: &str,
        expected: ProbePhase,
    ) -> Result<(), HostError> {
        self.transition_execution_publication(operation_id, expected, ProbePhase::Quarantined)
            .await
    }

    /// Abort only a prepared publication whose Docker request did not start.
    pub(crate) async fn abort_prepared_execution_publication(
        &self,
        operation_id: &str,
    ) -> Result<(), HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'aborted' WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND phase = 'prepared' AND container_id IS NULL",
                    [operation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            super::lease::release_prepared(connection, operation_id).await
        })
        .await
    }

    /// Charge an unresolved publication to restart reconciliation.
    pub(crate) async fn begin_execution_publication_recovery(
        &self,
        operation_id: &str,
    ) -> Result<ProbePhase, HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let mut rows = connection
                .query(
                    "SELECT phase FROM resource_probe_operations WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND operation_name = 'velnor-execution-publication-' || operation_id",
                    [operation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Ownership)?;
            let phase = ProbePhase::parse(
                &row.get::<String>(0).map_err(|_| HostError::Journal)?,
            )?;
            if matches!(phase, ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted) {
                return Err(HostError::Ownership);
            }
            super::lease::mark_recovering(connection, operation_id).await?;
            Ok(phase)
        })
        .await
    }
}

async fn verify_publication_role(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_name FROM resource_probe_operations WHERE operation_id = ?1 AND helper_role = 'execution-publication'",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Ownership)?;
    let operation_name = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    if operation_name != format!("velnor-execution-publication-{operation_id}") {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn publication_lease_state(phase: ProbePhase) -> Result<super::ProbeLeaseState, HostError> {
    match phase {
        ProbePhase::CreateRequested
        | ProbePhase::StartRequested
        | ProbePhase::WaitRequested
        | ProbePhase::LogsRequested
        | ProbePhase::StopRequested
        | ProbePhase::RemoveRequested
        | ProbePhase::Quarantined => Ok(super::ProbeLeaseState::Uncertain),
        ProbePhase::ContainerCreated
        | ProbePhase::Started
        | ProbePhase::Waited
        | ProbePhase::OutputObserved => Ok(super::ProbeLeaseState::Active),
        ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted => Err(HostError::Journal),
    }
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn digest_hex(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}
