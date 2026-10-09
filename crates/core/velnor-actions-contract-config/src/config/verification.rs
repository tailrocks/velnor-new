//! Closed declarations for isolated, credential-free verification jobs.

use super::ArtifactBuildOutput;
use super::artifact_build::validate_output_inventory;
use super::mise::is_valid_mise_task_name;
use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

/// Generated job-key prefix for a declared workflow verification task.
pub const VERIFICATION_TASK_JOB_PREFIX: &str = "task-";

/// One allowlisted task executed in its own least-privilege job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationTask {
    /// Stable lowercase identifier; the generated job ID is `task-{id}`.
    pub id: String,
    /// Explicit task capability. Other capability kinds require separate contracts.
    pub kind: VerificationTaskKind,
    /// Exact task name from the repository's locked Mise configuration.
    pub mise_task: String,
    /// OS and architecture used for this task.
    pub runner: VerificationRunner,
    /// Required per-job timeout in minutes.
    pub timeout_minutes: u16,
    /// Optional, bounded repository files to capture from the task.
    /// An empty inventory is equivalent to an omitted field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<ArtifactBuildOutput>,
}

/// Closed verification-only task kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationTaskKind {
    /// Repository-declared validation task whose body V1 does not inspect.
    ///
    /// Task authors and reviewers must keep this task free of Rust
    /// compilation; the typed declaration does not enforce that property.
    Verification,
}

/// Supported verification runner OS and architecture pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationRunner {
    /// GitHub-hosted Linux x64 runner.
    LinuxX64,
    /// GitHub-hosted macOS Apple ARM64 runner.
    MacosArm64,
}

/// Runner label for one typed verification platform.
impl VerificationRunner {
    /// Exact GitHub-hosted runner label.
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxX64 => "ubuntu-26.04",
            Self::MacosArm64 => "macos-15",
        }
    }

    /// Mise target triple for the runner's operating system and architecture.
    #[must_use]
    pub const fn mise_target(self) -> &'static str {
        match self {
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
        }
    }
}

impl VerificationTask {
    /// Validate task identifiers, task names, and bounded execution time.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_valid_verification_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_verification_task_id:{}", self.id),
            ));
        }
        if !is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.mise_task",
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if !(1..=360).contains(&self.timeout_minutes) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.timeout_minutes",
                format!("bad_timeout:{}", self.timeout_minutes),
            ));
        }
        if !self.outputs.is_empty() {
            if self.runner != VerificationRunner::LinuxX64 {
                return Err(ContractError::config(
                    file,
                    "workflow.tasks.runner",
                    "artifact_build_requires_linux_x64",
                ));
            }
            validate_output_inventory(&self.outputs, file, "workflow.tasks.outputs")?;
        }
        Ok(())
    }
}

/// True for an argv-safe task ID that cannot collide with generator-owned names.
#[must_use]
pub fn is_valid_verification_task_id(id: &str) -> bool {
    const RESERVED: [&str; 7] = [
        "actionlint",
        "candidate",
        "plan",
        "publish-baseline",
        "required",
        "task",
        "velnor-task",
    ];
    !id.is_empty()
        && id.len() <= 48
        && !RESERVED.contains(&id)
        && id.as_bytes()[0].is_ascii_lowercase()
        && (id.as_bytes()[id.len() - 1].is_ascii_lowercase()
            || id.as_bytes()[id.len() - 1].is_ascii_digit())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !id.contains("--")
}

#[cfg(test)]
mod tests;
