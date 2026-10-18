//! Durable intent and phase transitions for guest resource probes.

mod lease;
mod operations;
mod phase;
pub(crate) use lease::ProbeLeaseState;
pub(crate) use phase::ProbePhase;

/// Complete safe-to-persist identity, excluding the raw Docker root path.
#[derive(Debug, Clone)]
pub(crate) struct ProbeSeed {
    /// Random 128-bit lowercase hexadecimal operation ID.
    pub(crate) operation_id: String,
    /// Stable journal instance identity.
    pub(crate) instance_id: String,
    /// Selected Docker daemon identity.
    pub(crate) engine_id: String,
    /// Domain-separated digest of the canonical Docker root path.
    pub(crate) docker_root_digest: String,
    /// Exact source revision accepted by the image verifier.
    pub(crate) source_revision: String,
    /// Daemon-specific immutable runtime image reference.
    pub(crate) runtime_image_id: String,
    /// Digest binding source, authority, archive, platform, and store identity.
    pub(crate) image_binding_digest: String,
    /// Operation-scoped Docker name.
    pub(crate) operation_name: String,
    /// Canonical digest of the full create projection.
    pub(crate) projection_digest: String,
}

/// The only unresolved probe operation, if one is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeRow {
    /// Durable operation ID.
    pub(crate) operation_id: String,
    /// Stable journal instance identity.
    pub(crate) instance_id: String,
    /// Selected Docker daemon identity.
    pub(crate) engine_id: String,
    /// Domain-separated Docker root digest.
    pub(crate) docker_root_digest: String,
    /// Accepted probe source revision.
    pub(crate) source_revision: String,
    /// Daemon-specific immutable runtime image reference.
    pub(crate) runtime_image_id: String,
    /// Verified image binding digest.
    pub(crate) image_binding_digest: String,
    /// Exact operation-scoped Docker name.
    pub(crate) operation_name: String,
    /// Fixed controller-owned role.
    pub(crate) role: String,
    /// Canonical projection digest.
    pub(crate) projection_digest: String,
    /// Current durable external-effect phase.
    pub(crate) phase: ProbePhase,
    /// Immutable container ID, once the create response is committed.
    pub(crate) container_id: Option<String>,
    /// Durable shared-capacity lease state.
    pub(crate) lease_state: ProbeLeaseState,
}

#[cfg(test)]
#[path = "resource_probe/lease_tests.rs"]
mod tests;
