//! Closed declarations for isolated, credential-free verification jobs.

use super::mise::is_valid_mise_task_name;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

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
mod tests {
    use super::{
        VerificationRunner, VerificationTask, VerificationTaskKind, is_valid_mise_task_name,
        is_valid_verification_task_id,
    };

    fn task(id: &str, mise_task: &str, timeout_minutes: u16) -> VerificationTask {
        VerificationTask {
            id: id.to_owned(),
            kind: VerificationTaskKind::Verification,
            mise_task: mise_task.to_owned(),
            runner: VerificationRunner::LinuxX64,
            timeout_minutes,
        }
    }

    #[test]
    fn ids_and_mise_task_names_reject_shell_and_yaml_syntax() {
        for id in ["native-swift-format", "check1", "a-b-c"] {
            assert!(is_valid_verification_task_id(id), "{id}");
        }
        for id in ["", "Upper", "-start", "end-", "a--b", "required", "x/y"] {
            assert!(!is_valid_verification_task_id(id), "{id:?}");
        }
        for name in ["audit", "lint:strict", "tool_1.test"] {
            assert!(is_valid_mise_task_name(name), "{name}");
        }
        for name in ["", "--flag", "a b", "a/b", "${{ x }}", "a;id"] {
            assert!(!is_valid_mise_task_name(name), "{name:?}");
        }
    }

    #[test]
    fn timeout_is_bounded_and_runner_targets_are_platform_specific() {
        for timeout in [1, 360] {
            assert!(
                task("audit", "audit", timeout)
                    .validate("config.toml")
                    .is_ok()
            );
        }
        for timeout in [0, 361, u16::MAX] {
            assert!(
                task("audit", "audit", timeout)
                    .validate("config.toml")
                    .is_err()
            );
        }
        assert_eq!(
            VerificationRunner::LinuxX64.mise_target(),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            VerificationRunner::MacosArm64.mise_target(),
            "aarch64-apple-darwin"
        );
    }
}
