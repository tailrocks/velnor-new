//! Context digest creation and validation for publication operations.

use super::types::{
    ContextDigest, ExecutionPublicationContext, ExecutionPublicationSeed, ParentDigest,
};
use super::validation::{digest_hex, encode_hex, lower_hex};
use super::{MAX_PARENT_TEXT, MAX_RECOVERY_PROJECTION_BYTES};
use crate::{error::HostError, journal::VerifiedMetadataInitialization};
use sha2::{Digest, Sha256};

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
}
