//! Closed declarations for isolated, credential-free verification jobs.

use super::{MiseTaskSource, mise::is_valid_mise_task_name};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// One allowlisted task executed in its own least-privilege job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationTask {
    /// Stable lowercase identifier; the generated job ID is `task-{id}`.
    pub id: String,
    /// Exact task name from the repository's locked Mise configuration.
    pub mise_task: String,
    /// Exact Mise source file and task working directory.
    pub source: MiseTaskSource,
    /// OS and architecture used for this task.
    pub runner: VerificationRunner,
    /// Required per-job timeout in minutes.
    pub timeout_minutes: u16,
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
    /// Validate identifiers, source paths, task names, and bounded execution time.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !super::is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if !is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.mise_task",
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if !self.source.validate() {
            return Err(ContractError::config(
                file,
                "workflow.tasks.source",
                "bad_verification_source_or_working_directory",
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

#[cfg(test)]
mod tests {
    use super::{VerificationRunner, VerificationTask};
    use crate::config::MiseTaskSource;
    use crate::config::mise::is_valid_mise_task_name;

    fn task(id: &str, mise_task: &str, timeout_minutes: u16) -> VerificationTask {
        VerificationTask {
            id: id.to_owned(),
            mise_task: mise_task.to_owned(),
            source: MiseTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: VerificationRunner::LinuxX64,
            timeout_minutes,
        }
    }

    #[test]
    fn ids_and_mise_task_names_reject_shell_and_yaml_syntax() {
        for id in ["native-swift-format", "check1", "a-b-c"] {
            assert!(super::super::is_valid_workflow_task_id(id), "{id}");
        }
        for id in ["", "Upper", "-start", "end-", "a--b", "required", "x/y"] {
            assert!(!super::super::is_valid_workflow_task_id(id), "{id:?}");
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

    #[test]
    fn source_paths_are_canonical_and_working_directory_stays_under_config_root() {
        let valid = MiseTaskSource {
            mise_config: "native/mise.toml".to_owned(),
            working_directory: "native/apple".to_owned(),
        };
        assert_eq!(valid.mise_lock_path(), "native/mise.lock");
        assert_eq!(valid.rust_toolchain_path(), "native/rust-toolchain.toml");
        assert_eq!(valid.config_ceiling_directory(), ".");
        let mut valid_task = task("native-format", "format", 10);
        valid_task.source = valid;
        assert!(valid_task.validate("config.toml").is_ok());

        for (mise_config, working_directory) in [
            ("../mise.toml", ".."),
            ("/native/mise.toml", "native"),
            ("C:/native/mise.toml", "C:/native"),
            ("native\\mise.toml", "native"),
            ("native/../mise.toml", "native"),
            ("native/mise.toml", "other"),
            ("native/mise.toml", "native/../outside"),
            ("native/custom.toml", "native"),
            ("native//mise.toml", "native"),
            ("native/mise.toml", "native/"),
            ("native/mise.toml", "native/$HOME"),
        ] {
            let mut invalid = task("native-format", "format", 10);
            invalid.source = MiseTaskSource {
                mise_config: mise_config.to_owned(),
                working_directory: working_directory.to_owned(),
            };
            assert!(
                invalid.validate("config.toml").is_err(),
                "accepted {mise_config:?} from {working_directory:?}"
            );
        }
    }
}
