//! Bounded repository-owned jobs whose outputs are required build artifacts.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

use super::{VerificationRunner, is_valid_mise_task_name, is_valid_verification_task_id};

/// One isolated build task whose declared outputs must be uploaded and checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildTask {
    /// Stable lowercase task identity.
    pub id: String,
    /// Exact task name from the locked repository Mise configuration.
    pub mise_task: String,
    /// OS/architecture of this reproducible build lane.
    pub runner: VerificationRunner,
    /// Required task timeout in minutes.
    pub timeout_minutes: u16,
    /// Exact outputs expected from this task, sorted by output ID.
    pub outputs: Vec<ArtifactBuildOutput>,
}

/// One declared repository file output with a nonzero byte ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBuildOutput {
    /// Stable output ID used in evidence records.
    pub id: String,
    /// Repository-relative POSIX path; no globs or traversal.
    pub path: String,
    /// Maximum raw bytes accepted after generation.
    pub max_bytes: u64,
}

impl ArtifactBuildTask {
    /// Validate a task and all output declarations before planning.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_valid_verification_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.artifact_tasks.id",
                format!("bad_artifact_task_id:{}", self.id),
            ));
        }
        if !is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                "workflow.artifact_tasks.mise_task",
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if self.runner != VerificationRunner::LinuxX64 {
            return Err(ContractError::config(
                file,
                "workflow.artifact_tasks.runner",
                "artifact_build_requires_linux_x64",
            ));
        }
        if !(1..=360).contains(&self.timeout_minutes) {
            return Err(ContractError::config(
                file,
                "workflow.artifact_tasks.timeout_minutes",
                format!("bad_timeout:{}", self.timeout_minutes),
            ));
        }
        validate_output_inventory(&self.outputs, file, "workflow.artifact_tasks.outputs")
    }
}

impl ArtifactBuildOutput {
    /// Validate one output identity, source, and required size ceiling.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.validate_at(file, "workflow.artifact_tasks.outputs")
    }

    fn validate_at(&self, file: &str, field: &str) -> Result<(), ContractError> {
        if !is_valid_verification_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                format!("{field}.id"),
                format!("bad_artifact_output_id:{}", self.id),
            ));
        }
        if self.max_bytes == 0 {
            return Err(ContractError::config(
                file,
                format!("{field}.max_bytes"),
                "must_be_positive",
            ));
        }
        if !safe_output_path(&self.path) {
            return Err(ContractError::config(
                file,
                format!("{field}.path"),
                "unsafe_artifact_path",
            ));
        }
        Ok(())
    }
}

/// Validate one nonempty, sorted, unambiguous output inventory.
pub(super) fn validate_output_inventory(
    outputs: &[ArtifactBuildOutput],
    file: &str,
    field: &str,
) -> Result<(), ContractError> {
    if outputs.is_empty() {
        return Err(ContractError::config(file, field, "empty_output_inventory"));
    }
    let mut previous: Option<&str> = None;
    let mut paths = std::collections::BTreeSet::new();
    for output in outputs {
        output.validate_at(file, field)?;
        if previous.is_some_and(|value| value >= output.id.as_str()) {
            let issue = if previous == Some(output.id.as_str()) {
                format!("duplicate_artifact_output:{}", output.id)
            } else {
                "artifact_outputs_must_be_sorted_by_id".to_owned()
            };
            return Err(ContractError::config(file, field, issue));
        }
        if !paths.insert(output.path.as_str()) {
            return Err(ContractError::config(
                file,
                format!("{field}.path"),
                format!("duplicate_artifact_output_path:{}", output.path),
            ));
        }
        previous = Some(output.id.as_str());
    }
    Ok(())
}

fn safe_output_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains("${{")
        && !path.chars().any(char::is_control)
        && !path
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b':'))
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

#[cfg(test)]
mod tests;
