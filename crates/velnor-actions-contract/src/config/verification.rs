//! Closed declarations for isolated, credential-free verification jobs.

use super::mise::is_valid_mise_task_name;
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
    pub source: VerificationTaskSource,
    /// OS and architecture used for this task.
    pub runner: VerificationRunner,
    /// Required per-job timeout in minutes.
    pub timeout_minutes: u16,
}

/// Repository-local source selected for one verification task.
///
/// The config file is the task's Mise root. The working directory must be
/// that directory or one of its descendants so Mise cannot resolve the task
/// from an unrelated repository config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationTaskSource {
    /// Repository-relative `mise.toml` that declares the task.
    pub mise_config: String,
    /// Repository-relative directory from which the task is invoked.
    pub working_directory: String,
}

impl VerificationTaskSource {
    /// `mise.lock` beside the declared config, if present.
    #[must_use]
    pub fn mise_lock_path(&self) -> String {
        self.sibling("mise.lock")
    }

    /// Idiomatic Rust toolchain file beside the declared config, if present.
    #[must_use]
    pub fn rust_toolchain_path(&self) -> String {
        self.sibling("rust-toolchain.toml")
    }

    /// Directory immediately above the source config directory. Mise uses
    /// this as its ceiling so parent repository configs cannot be discovered.
    #[must_use]
    pub fn config_ceiling_directory(&self) -> String {
        let config_dir = self.config_directory();
        if config_dir == "." {
            "..".to_owned()
        } else {
            config_dir
                .rsplit_once('/')
                .map_or_else(|| ".".to_owned(), |(parent, _)| parent.to_owned())
        }
    }

    fn config_directory(&self) -> &str {
        self.mise_config
            .strip_suffix("/mise.toml")
            .filter(|directory| !directory.is_empty())
            .unwrap_or(".")
    }

    fn sibling(&self, name: &str) -> String {
        if self.config_directory() == "." {
            name.to_owned()
        } else {
            format!("{}/{name}", self.config_directory())
        }
    }

    fn validate(&self) -> bool {
        valid_repository_path(&self.mise_config, false)
            && self.mise_config.ends_with("mise.toml")
            && valid_repository_path(&self.working_directory, true)
            && self.working_directory.len() <= 1024
            && self
                .mise_config
                .strip_suffix("mise.toml")
                .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
            && (self.config_directory() == "."
                || self.working_directory == self.config_directory()
                || self
                    .working_directory
                    .strip_prefix(self.config_directory())
                    .is_some_and(|suffix| suffix.starts_with('/')))
    }
}

fn valid_repository_path(path: &str, root_allowed: bool) -> bool {
    if path == "." {
        return root_allowed;
    }
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.ends_with('/')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
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
    use super::{VerificationRunner, VerificationTask, VerificationTaskSource};
    use crate::config::mise::is_valid_mise_task_name;

    fn task(id: &str, mise_task: &str, timeout_minutes: u16) -> VerificationTask {
        VerificationTask {
            id: id.to_owned(),
            mise_task: mise_task.to_owned(),
            source: VerificationTaskSource {
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
        let valid = VerificationTaskSource {
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
            invalid.source = VerificationTaskSource {
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
