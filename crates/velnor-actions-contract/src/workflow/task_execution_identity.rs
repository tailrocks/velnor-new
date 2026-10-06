//! Planned execution dimensions; a run becomes proof only after qualification.

use serde::{Deserialize, Serialize};

use crate::{ContractError, Digest, ManifestTaskProof};

/// Source-bound execution identity without an originating execution run.
///
/// These dimensions describe the planned task, including tasks whose input
/// closure is unresolved. They do not assert that unknown inputs are complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskExecutionIdentityUnchecked")]
pub struct TaskExecutionIdentity {
    graph_digest: Digest,
    toolchain_id: Digest,
    mbx_digest: Digest,
    platform_id: Digest,
    profile: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskExecutionIdentityUnchecked {
    graph_digest: String,
    toolchain_id: String,
    mbx_digest: String,
    platform_id: String,
    profile: String,
}

impl TaskExecutionIdentity {
    /// Construct validated dimensions from the planner's resolved identity.
    /// # Errors
    /// Rejects malformed digests and invalid execution profiles.
    pub fn new(
        graph_digest: &str,
        toolchain_id: &str,
        mbx_digest: &str,
        platform_id: &str,
        profile: &str,
    ) -> Result<Self, ContractError> {
        crate::cachekey::validate_semantic_text("profile", profile)?;
        Ok(Self {
            graph_digest: Digest::parse(graph_digest)?,
            toolchain_id: Digest::parse(toolchain_id)?,
            mbx_digest: Digest::parse(mbx_digest)?,
            platform_id: Digest::parse(platform_id)?,
            profile: profile.to_owned(),
        })
    }

    /// Workspace graph digest.
    #[must_use]
    pub fn graph_digest(&self) -> &str {
        self.graph_digest.as_str()
    }

    /// Catalog-bound toolchain identity.
    #[must_use]
    pub fn toolchain_id(&self) -> &str {
        self.toolchain_id.as_str()
    }

    /// Compile-driver and applicable pinned MBX identity.
    #[must_use]
    pub fn mbx_digest(&self) -> &str {
        self.mbx_digest.as_str()
    }

    /// Requested execution platform identity.
    #[must_use]
    pub fn platform_id(&self) -> &str {
        self.platform_id.as_str()
    }

    /// Exact execution profile.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Revalidate stored dimensions.
    /// # Errors
    /// Rejects an invalid execution profile; digests are validated types.
    pub fn validate(&self) -> Result<(), ContractError> {
        crate::cachekey::validate_semantic_text("profile", &self.profile)
    }

    /// Match every execution dimension with the recorded originating proof.
    #[must_use]
    pub fn matches_proof(&self, proof: &ManifestTaskProof) -> bool {
        self.graph_digest() == proof.graph_digest()
            && self.toolchain_id() == proof.toolchain_id()
            && self.mbx_digest() == proof.mbx_digest()
            && self.platform_id() == proof.platform_id()
            && self.profile() == proof.profile()
    }
}

impl TryFrom<TaskExecutionIdentityUnchecked> for TaskExecutionIdentity {
    type Error = ContractError;
    fn try_from(raw: TaskExecutionIdentityUnchecked) -> Result<Self, Self::Error> {
        Self::new(
            &raw.graph_digest,
            &raw.toolchain_id,
            &raw.mbx_digest,
            &raw.platform_id,
            &raw.profile,
        )
    }
}

#[cfg(test)]
#[path = "task_execution_identity_tests.rs"]
mod tests;
