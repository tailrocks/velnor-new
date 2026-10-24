//! Typed publication identities and recovery records.

use serde::Serialize;

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
    pub(super) kind: PublicationKind,
    pub(super) operation_id: i64,
    pub(super) reservation_id: i64,
    pub(super) connection_id: String,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: Option<i64>,
    pub(super) actual_request_id: Option<i64>,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) metadata_volume_name: String,
    pub(super) metadata_volume_id: String,
    pub(super) metadata_layout_version: i64,
    pub(super) metadata_layout_digest: String,
    pub(super) guest_boot_id: String,
    pub(super) payload_sha256: String,
    pub(super) claim_digest: String,
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
    pub(super) const fn as_str(self) -> &'static str {
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
    pub(super) operation_id: String,
    pub(super) instance_id: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) operation_name: String,
    pub(super) source_revision: String,
    pub(super) runtime_image_id: String,
    pub(super) image_binding_digest: String,
    pub(super) projection_digest: String,
    pub(super) recovery_projection_json: String,
    pub(super) recovery_projection_sha256: String,
    pub(super) parent: ExecutionPublicationParent,
    pub(super) context_digest: String,
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
#[derive(Serialize)]
pub(super) struct ContextDigest<'a> {
    pub(super) operation_id: &'a str,
    pub(super) instance_id: &'a str,
    pub(super) engine_id: &'a str,
    pub(super) docker_root_digest: &'a str,
    pub(super) operation_name: &'a str,
    pub(super) source_revision: &'a str,
    pub(super) runtime_image_id: &'a str,
    pub(super) image_binding_digest: &'a str,
    pub(super) projection_digest: &'a str,
    pub(super) recovery_projection_sha256: &'a str,
    pub(super) parent: ParentDigest<'a>,
}

#[derive(Serialize)]
pub(super) struct ParentDigest<'a> {
    pub(super) kind: &'a str,
    pub(super) operation_id: i64,
    pub(super) reservation_id: i64,
    pub(super) connection_id: &'a str,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: Option<i64>,
    pub(super) actual_request_id: Option<i64>,
    pub(super) runner_id: i64,
    pub(super) runner_name: &'a str,
    pub(super) runner_container_id: &'a str,
    pub(super) dind_container_id: &'a str,
    pub(super) engine_id: &'a str,
    pub(super) docker_root_digest: &'a str,
    pub(super) metadata_volume_name: &'a str,
    pub(super) metadata_volume_id: &'a str,
    pub(super) metadata_layout_version: i64,
    pub(super) metadata_layout_digest: &'a str,
    pub(super) guest_boot_id: &'a str,
    pub(super) payload_sha256: &'a str,
    pub(super) claim_digest: &'a str,
}
