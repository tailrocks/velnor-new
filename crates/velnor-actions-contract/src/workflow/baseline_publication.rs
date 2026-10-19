//! Types for published baseline evidence.

use crate::cachekey::validate_semantic_text;
use crate::canonical::Digest;
use crate::errors::ContractError;
use crate::ids::TaskId;
use serde::{Deserialize, Serialize};

/// One task proof in a published baseline manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "ManifestTaskProofUnchecked")]
pub struct ManifestTaskProof {
    task_id: TaskId,
    task_digest: Digest,
    input_digest: Digest,
    graph_digest: Digest,
    toolchain_id: Digest,
    mbx_digest: Digest,
    platform_id: Digest,
    profile: String,
    proof_run_id: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTaskProofUnchecked {
    task_id: String,
    task_digest: String,
    input_digest: String,
    graph_digest: String,
    toolchain_id: String,
    mbx_digest: String,
    platform_id: String,
    profile: String,
    proof_run_id: u64,
}
impl ManifestTaskProof {
    /// Build one task proof from validated values.
    /// # Errors
    #[expect(
        clippy::too_many_arguments,
        reason = "proof carries nine validated inputs at once"
    )]
    pub fn new(
        task_id: &str,
        task_digest: &str,
        input_digest: &str,
        graph_digest: &str,
        toolchain_id: &str,
        mbx_digest: &str,
        platform_id: &str,
        profile: &str,
        proof_run_id: u64,
    ) -> Result<Self, ContractError> {
        validate_semantic_text("profile", profile)?;
        check_proof_id(proof_run_id, "proof_run_id")?;
        Ok(Self {
            task_id: TaskId::parse(task_id)?,
            task_digest: Digest::parse(task_digest)?,
            input_digest: Digest::parse(input_digest)?,
            graph_digest: Digest::parse(graph_digest)?,
            toolchain_id: Digest::parse(toolchain_id)?,
            mbx_digest: Digest::parse(mbx_digest)?,
            platform_id: Digest::parse(platform_id)?,
            profile: profile.to_owned(),
            proof_run_id,
        })
    }

    /// Return the task ID.
    #[must_use]
    pub fn task_id(&self) -> &str {
        self.task_id.as_str()
    }

    /// Return the task digest.
    #[must_use]
    pub fn task_digest(&self) -> &str {
        self.task_digest.as_str()
    }

    /// Return the input digest.
    #[must_use]
    pub fn input_digest(&self) -> &str {
        self.input_digest.as_str()
    }

    /// Return the graph digest.
    #[must_use]
    pub fn graph_digest(&self) -> &str {
        self.graph_digest.as_str()
    }

    /// Return the toolchain digest.
    #[must_use]
    pub fn toolchain_id(&self) -> &str {
        self.toolchain_id.as_str()
    }

    /// Return the MBX digest.
    #[must_use]
    pub fn mbx_digest(&self) -> &str {
        self.mbx_digest.as_str()
    }

    /// Return the platform digest.
    #[must_use]
    pub fn platform_id(&self) -> &str {
        self.platform_id.as_str()
    }

    /// Return the profile name.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Return the proof run ID.
    #[must_use]
    pub fn proof_run_id(&self) -> u64 {
        self.proof_run_id
    }

    /// Check the stored proof values again.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_semantic_text("profile", &self.profile)?;
        check_proof_id(self.proof_run_id, "proof_run_id")
    }
}
impl TryFrom<ManifestTaskProofUnchecked> for ManifestTaskProof {
    type Error = ContractError;
    fn try_from(raw: ManifestTaskProofUnchecked) -> Result<Self, Self::Error> {
        Self::new(
            &raw.task_id,
            &raw.task_digest,
            &raw.input_digest,
            &raw.graph_digest,
            &raw.toolchain_id,
            &raw.mbx_digest,
            &raw.platform_id,
            &raw.profile,
            raw.proof_run_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ManifestTaskProof;
    use crate::canonical::digest_b3;

    #[test]
    fn published_task_proof_keeps_schema_two_fields() {
        let digest = digest_b3(b"task proof");
        let proof = ManifestTaskProof::new(
            "stack/rust/root/test/default",
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            &digest,
            "default",
            12,
        )
        .expect("valid proof");
        let value = serde_json::to_value(proof).expect("serialize");

        assert_eq!(value["task_id"], "stack/rust/root/test/default");
        assert_eq!(value["proof_run_id"], 12);
        assert_eq!(value.as_object().expect("object").len(), 9);
    }
}

/// Check that a proof run ID is nonzero.
fn check_proof_id(id: u64, field: &'static str) -> Result<(), ContractError> {
    if id == 0 {
        return Err(ContractError::identity(field, "missing_proof_id"));
    }
    Ok(())
}
