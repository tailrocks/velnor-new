//! Bounded output digest construction and validation.

use velnor_actions_contract::canonical::{digest_b3, validate_digest};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::ArtifactBuildTask;

use super::identity::{artifact_name, validate_identity};
use super::{ArtifactBuildFile, ArtifactBuildIdentity, ArtifactBuildResult};

/// Build a result from bytes read from declared outputs.
///
/// The caller must open declared paths beneath its checkout without following
/// symlinks and bound each read to the plan's `max_bytes` before allocating.
/// This function verifies their exact content inventory and computes digests.
/// # Errors
pub fn export_artifact_result(
    identity: ArtifactBuildIdentity,
    task: &ArtifactBuildTask,
    files_by_id: &[(String, Vec<u8>)],
) -> Result<ArtifactBuildResult, ContractError> {
    task.validate("plan")?;
    validate_identity(&identity)?;
    if identity.task_id != task.id {
        return Err(ContractError::identity("artifact.task_id", "task_mismatch"));
    }
    let expected_name = artifact_name(&identity)?;
    let mut outputs = Vec::with_capacity(files_by_id.len());
    for (index, (output_id, bytes)) in files_by_id.iter().enumerate() {
        if bytes.is_empty() {
            return Err(ContractError::identity(
                "artifact.output",
                format!("empty_output:{output_id}"),
            ));
        }
        let expected = task
            .outputs
            .get(index)
            .ok_or_else(|| ContractError::identity("artifact.outputs", "unexpected_output"))?;
        if expected.id != *output_id {
            return Err(ContractError::identity(
                "artifact.outputs",
                "output_inventory_mismatch",
            ));
        }
        let size_bytes = u64::try_from(bytes.len())
            .map_err(|_| ContractError::identity("artifact.output", "size_overflow"))?;
        if size_bytes > expected.max_bytes {
            return Err(ContractError::identity(
                "artifact.output",
                format!("output_exceeds_declared_limit:{output_id}"),
            ));
        }
        outputs.push(ArtifactBuildFile {
            output_id: output_id.clone(),
            path: expected.path.clone(),
            size_bytes,
            digest: digest_b3(bytes),
        });
    }
    if outputs.len() != task.outputs.len() {
        return Err(ContractError::identity(
            "artifact.outputs",
            "missing_output",
        ));
    }
    let result = ArtifactBuildResult {
        schema: ArtifactBuildResult::SCHEMA,
        artifact_name: expected_name,
        identity,
        outputs,
    };
    result.validate_for(task, &result.identity)?;
    Ok(result)
}

impl ArtifactBuildResult {
    /// Result schema version.
    pub const SCHEMA: u32 = 1;

    /// Validate identity, deterministic name, exact output set, and bounds.
    /// # Errors
    pub fn validate_for(
        &self,
        task: &ArtifactBuildTask,
        expected: &ArtifactBuildIdentity,
    ) -> Result<(), ContractError> {
        task.validate("plan")?;
        validate_identity(expected)?;
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "artifact.schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        if self.identity != *expected || expected.task_id != task.id {
            return Err(ContractError::identity(
                "artifact.identity",
                "identity_mismatch",
            ));
        }
        if self.artifact_name != artifact_name(expected)? {
            return Err(ContractError::identity(
                "artifact.name",
                "artifact_name_mismatch",
            ));
        }
        if self.outputs.len() != task.outputs.len() {
            return Err(ContractError::identity(
                "artifact.outputs",
                "output_inventory_mismatch",
            ));
        }
        for (actual, declared) in self.outputs.iter().zip(&task.outputs) {
            if actual.output_id != declared.id || actual.path != declared.path {
                return Err(ContractError::identity(
                    "artifact.outputs",
                    "output_inventory_mismatch",
                ));
            }
            if actual.size_bytes == 0 || actual.size_bytes > declared.max_bytes {
                return Err(ContractError::identity(
                    "artifact.output",
                    format!("invalid_output_size:{}", declared.id),
                ));
            }
            validate_digest(&actual.digest)?;
        }
        Ok(())
    }
}
