//! This module reads the native reservation identity for one completed launch.

/// This record keeps the immutable identity and cleanup progress for one native launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeCompletionReservation {
    reservation_id: i64,
    intent_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    remote_cleanup_proven: bool,
    local_cleanup_evidence: LocalCleanupEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalCleanupEvidence {
    storage: bool,
    network: bool,
    local: bool,
}

impl NativeCompletionReservation {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
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

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) const fn remote_cleanup_proven(&self) -> bool {
        self.remote_cleanup_proven
    }

    pub(crate) const fn storage_cleanup_proven(&self) -> bool {
        self.local_cleanup_evidence.storage
    }

    pub(crate) const fn network_cleanup_proven(&self) -> bool {
        self.local_cleanup_evidence.network
    }

    pub(crate) const fn local_cleanup_proven(&self) -> bool {
        self.local_cleanup_evidence.local
    }
}

#[path = "native_reservation/query.rs"]
mod query;

#[cfg(test)]
#[path = "native_reservation/tests.rs"]
mod tests;
