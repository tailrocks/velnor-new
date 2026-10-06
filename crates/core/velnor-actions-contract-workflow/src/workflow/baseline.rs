//! Trusted-baseline evidence records.
//!
//! Proof types are constructible only via validating constructors or
//! validated deserialization. Field literals cannot forge them: every
//! field is private and every constructor proves its inputs.
use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::{ArtifactId, TaskId};
/// Baseline evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "PlanBaselineUnchecked")]
pub struct PlanBaseline {
    status: BaselineStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifact_name: Option<ArtifactId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest_digest: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}
/// Baseline status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineStatus {
    /// Baseline evidence used.
    Used,
    /// No usable baseline.
    Unavailable,
}
/// Baseline proof for a covered obligation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "BaselineProofUnchecked")]
pub struct BaselineProof {
    source_commit: String,
    run_id: u64,
    artifact_id: u64,
    artifact_name: ArtifactId,
    manifest_digest: Digest,
}
/// Per-task manifest proof entry (par §5).
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
struct PlanBaselineUnchecked {
    status: BaselineStatus,
    #[serde(default)]
    base_commit: Option<String>,
    #[serde(default)]
    run_id: Option<u64>,
    #[serde(default)]
    artifact_id: Option<u64>,
    #[serde(default)]
    artifact_name: Option<String>,
    #[serde(default)]
    manifest_digest: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineProofUnchecked {
    source_commit: String,
    run_id: u64,
    artifact_id: u64,
    artifact_name: String,
    manifest_digest: String,
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
impl PlanBaseline {
    /// Build a `Used` baseline with complete validated evidence.
    /// # Errors
    pub fn used(
        base_commit: &str,
        run_id: u64,
        artifact_id: u64,
        artifact_name: &str,
        manifest_digest: &str,
    ) -> Result<Self, ContractError> {
        check_commit(base_commit)?;
        check_proof_id(run_id, "run_id")?;
        check_proof_id(artifact_id, "artifact_id")?;
        Ok(Self {
            status: BaselineStatus::Used,
            base_commit: Some(base_commit.to_owned()),
            run_id: Some(run_id),
            artifact_id: Some(artifact_id),
            artifact_name: Some(ArtifactId::parse(artifact_name)?),
            manifest_digest: Some(Digest::parse(manifest_digest)?),
            reason: None,
        })
    }

    /// Build an `Unavailable` baseline with an optional reason.
    /// # Errors
    pub fn unavailable(reason: Option<&str>) -> Result<Self, ContractError> {
        if reason.is_some_and(str::is_empty) {
            return Err(ContractError::identity("baseline.reason", "empty_reason"));
        }
        Ok(Self {
            status: BaselineStatus::Unavailable,
            base_commit: None,
            run_id: None,
            artifact_id: None,
            artifact_name: None,
            manifest_digest: None,
            reason: reason.map(str::to_owned),
        })
    }

    /// Mark unavailable, clearing any stale evidence.
    /// # Errors
    pub fn mark_unavailable(&mut self, reason: &str) -> Result<(), ContractError> {
        *self = Self::unavailable(Some(reason))?;
        Ok(())
    }

    /// Baseline status.
    #[must_use]
    pub fn status(&self) -> BaselineStatus {
        self.status
    }

    /// Unavailable reason.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}
impl TryFrom<PlanBaselineUnchecked> for PlanBaseline {
    type Error = ContractError;
    fn try_from(raw: PlanBaselineUnchecked) -> Result<Self, Self::Error> {
        match raw.status {
            BaselineStatus::Used => {
                if raw.reason.is_some() {
                    return Err(ContractError::identity("baseline", "used_with_reason"));
                }
                Self::used(
                    raw.base_commit.as_deref().unwrap_or_default(),
                    raw.run_id.unwrap_or_default(),
                    raw.artifact_id.unwrap_or_default(),
                    raw.artifact_name.as_deref().unwrap_or_default(),
                    raw.manifest_digest.as_deref().unwrap_or_default(),
                )
            }
            BaselineStatus::Unavailable => {
                let stale = raw.base_commit.is_some()
                    || raw.run_id.is_some()
                    || raw.artifact_id.is_some()
                    || raw.artifact_name.is_some()
                    || raw.manifest_digest.is_some();
                if stale {
                    return Err(ContractError::identity("baseline", "stale_evidence"));
                }
                Self::unavailable(raw.reason.as_deref())
            }
        }
    }
}
impl BaselineProof {
    /// Build a proof from validated commit, IDs, artifact name, and digest.
    /// # Errors
    pub fn new(
        source_commit: &str,
        run_id: u64,
        artifact_id: u64,
        artifact_name: &str,
        manifest_digest: &str,
    ) -> Result<Self, ContractError> {
        check_commit(source_commit)?;
        check_proof_id(run_id, "run_id")?;
        check_proof_id(artifact_id, "artifact_id")?;
        Ok(Self {
            source_commit: source_commit.to_owned(),
            run_id,
            artifact_id,
            artifact_name: ArtifactId::parse(artifact_name)?,
            manifest_digest: Digest::parse(manifest_digest)?,
        })
    }

    /// Source commit of the proof.
    #[must_use]
    pub fn source_commit(&self) -> &str {
        &self.source_commit
    }

    /// Numeric GitHub run ID.
    #[must_use]
    pub fn run_id(&self) -> u64 {
        self.run_id
    }

    /// Numeric GitHub artifact ID.
    #[must_use]
    pub fn artifact_id(&self) -> u64 {
        self.artifact_id
    }

    /// Derived baseline artifact name.
    #[must_use]
    pub fn artifact_name(&self) -> &str {
        self.artifact_name.as_str()
    }

    /// Proof manifest digest.
    #[must_use]
    pub fn manifest_digest(&self) -> &str {
        self.manifest_digest.as_str()
    }

    /// Re-validate stored proof values (construction already validated).
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_commit(&self.source_commit)?;
        check_proof_id(self.run_id, "run_id")?;
        check_proof_id(self.artifact_id, "artifact_id")
    }
}
impl TryFrom<BaselineProofUnchecked> for BaselineProof {
    type Error = ContractError;
    fn try_from(raw: BaselineProofUnchecked) -> Result<Self, Self::Error> {
        Self::new(
            &raw.source_commit,
            raw.run_id,
            raw.artifact_id,
            &raw.artifact_name,
            &raw.manifest_digest,
        )
    }
}
impl ManifestTaskProof {
    /// Build a task proof from validated IDs, digests, and profile.
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
        velnor_actions_contract::cachekey::validate_semantic_text("profile", profile)?;
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

    /// Task ID.
    #[must_use]
    pub fn task_id(&self) -> &str {
        self.task_id.as_str()
    }

    /// Task digest.
    #[must_use]
    pub fn task_digest(&self) -> &str {
        self.task_digest.as_str()
    }

    /// Input digest.
    #[must_use]
    pub fn input_digest(&self) -> &str {
        self.input_digest.as_str()
    }

    /// Graph digest.
    #[must_use]
    pub fn graph_digest(&self) -> &str {
        self.graph_digest.as_str()
    }

    /// Toolchain identity digest.
    #[must_use]
    pub fn toolchain_id(&self) -> &str {
        self.toolchain_id.as_str()
    }

    /// MBX digest.
    #[must_use]
    pub fn mbx_digest(&self) -> &str {
        self.mbx_digest.as_str()
    }

    /// Platform identity digest.
    #[must_use]
    pub fn platform_id(&self) -> &str {
        self.platform_id.as_str()
    }

    /// Profile name.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Numeric proof run ID.
    #[must_use]
    pub fn proof_run_id(&self) -> u64 {
        self.proof_run_id
    }

    /// Re-validate stored proof values (construction already validated).
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        velnor_actions_contract::cachekey::validate_semantic_text("profile", &self.profile)?;
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
/// Check a 40-char lowercase hex source commit.
fn check_commit(commit: &str) -> Result<(), ContractError> {
    if commit.len() == 40 && velnor_actions_contract::ids::is_lower_hex(commit) {
        Ok(())
    } else {
        Err(ContractError::identity("source_commit", "malformed_commit"))
    }
}
/// Check a nonzero numeric proof ID.
fn check_proof_id(id: u64, field: &'static str) -> Result<(), ContractError> {
    if id == 0 {
        return Err(ContractError::identity(field, "missing_proof_id"));
    }
    Ok(())
}
