//! Closed declarations for native build-capable Mise tasks.

use crate::errors::ContractError;

/// One allowlisted task that may compile repository code in a native job.
///
/// Build tasks are an explicit variant in the same workflow task collection as
/// compile-free verification tasks. They declare a runner, tool closure, time
/// limit, and per-tool parallelism bounds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildTask {
    /// Stable lowercase identifier; the generated job ID is `task-{id}`.
    pub id: String,
    /// Exact task name from the repository's locked Mise configuration.
    pub mise_task: String,
    /// Sorted, explicit closure of tool keys consumed by this task.
    pub tools: Vec<String>,
    /// Native OS and architecture used for this build task.
    pub runner: BuildTaskRunner,
    /// Required per-job timeout in minutes.
    pub timeout_minutes: u16,
    /// Maximum concurrent Cargo build jobs (1 or 2).
    pub cargo_build_jobs: u8,
    /// Maximum concurrent Nextest test threads (1 or 2).
    pub nextest_test_threads: u8,
}

/// Supported build-task runner OS and architecture pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildTaskRunner {
    /// GitHub-hosted Apple ARM64 macOS 26 runner.
    #[serde(rename = "macos-26-arm64")]
    Macos26Arm64,
}

impl BuildTaskRunner {
    /// Exact GitHub-hosted runner label.
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::Macos26Arm64 => "macos-26",
        }
    }

    /// Mise target triple for the runner's operating system and architecture.
    #[must_use]
    pub const fn mise_target(self) -> &'static str {
        match self {
            Self::Macos26Arm64 => "aarch64-apple-darwin",
        }
    }
}

impl BuildTask {
    /// Validate names, the declared tool closure, timeout, and resource bounds.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !super::is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if self.mise_task.len() > 64 || !crate::config::is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.mise_task",
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if self.tools.is_empty() || self.tools.len() > 32 {
            return Err(ContractError::config(
                file,
                "workflow.tasks.tools",
                "build_task_tools_out_of_bounds",
            ));
        }
        let mut previous = None;
        for tool in &self.tools {
            if !is_valid_build_tool_key(tool) {
                return Err(ContractError::config(
                    file,
                    "workflow.tasks.tools",
                    format!("bad_build_task_tool:{tool}"),
                ));
            }
            if previous.is_some_and(|name: &str| name >= tool.as_str()) {
                let problem = if previous == Some(tool.as_str()) {
                    format!("duplicate_build_task_tool:{tool}")
                } else {
                    "build_task_tools_must_be_sorted".to_owned()
                };
                return Err(ContractError::config(file, "workflow.tasks.tools", problem));
            }
            previous = Some(tool.as_str());
        }
        for required in ["mr-boxington", "rust"] {
            if self
                .tools
                .binary_search_by(|tool| tool.as_str().cmp(required))
                .is_err()
            {
                return Err(ContractError::config(
                    file,
                    "workflow.tasks.tools",
                    format!("missing_required_build_task_tool:{required}"),
                ));
            }
        }
        if !(1..=180).contains(&self.timeout_minutes) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.timeout_minutes",
                format!("bad_timeout:{}", self.timeout_minutes),
            ));
        }
        for (field, value) in [
            ("cargo_build_jobs", self.cargo_build_jobs),
            ("nextest_test_threads", self.nextest_test_threads),
        ] {
            if !(1..=2).contains(&value) {
                return Err(ContractError::config(
                    file,
                    format!("workflow.tasks.{field}"),
                    format!("outside_native_runner_bound:{value}"),
                ));
            }
        }
        Ok(())
    }
}

/// True for a bounded literal key that may name one Mise tool selector.
#[must_use]
pub fn is_valid_build_tool_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key.as_bytes()[0].is_ascii_alphanumeric()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::{BuildTask, BuildTaskRunner, is_valid_build_tool_key};

    fn build_task(id: &str, mise_task: &str) -> BuildTask {
        BuildTask {
            id: id.to_owned(),
            mise_task: mise_task.to_owned(),
            tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 120,
            cargo_build_jobs: 2,
            nextest_test_threads: 2,
        }
    }

    #[test]
    fn names_and_native_resource_limits_are_bounded() {
        for id in ["native-desktop", "macos26-ci", "desktop1"] {
            assert!(super::super::is_valid_workflow_task_id(id), "{id}");
        }
        for id in ["", "Upper", "-start", "end-", "a--b", "required", "x/y"] {
            assert!(!super::super::is_valid_workflow_task_id(id), "{id:?}");
        }

        let task = build_task("native-desktop", "desktop-ci");
        assert!(task.validate("config.toml").is_ok());
        assert_eq!(task.runner.runs_on(), "macos-26");
        assert_eq!(task.runner.mise_target(), "aarch64-apple-darwin");

        let mut invalid = task.clone();
        invalid.timeout_minutes = 181;
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = task.clone();
        invalid.cargo_build_jobs = 3;
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = task.clone();
        invalid.nextest_test_threads = 0;
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = task;
        invalid.mise_task = "desktop-ci;id".to_owned();
        assert!(invalid.validate("config.toml").is_err());
        let invalid = build_task("native-desktop", &"a".repeat(65));
        assert!(invalid.validate("config.toml").is_err());
    }

    #[test]
    fn tool_closure_is_safe_sorted_bounded_and_requires_mbx() {
        for key in ["actionlint", "aqua:example/tool", "github:boltffi/boltffi"] {
            assert!(is_valid_build_tool_key(key), "{key}");
        }
        for key in [
            "",
            "-prefix",
            "tool.name",
            "tool name",
            "tool;true",
            "../tool",
        ] {
            assert!(!is_valid_build_tool_key(key), "{key:?}");
        }

        let mut task = build_task("native-desktop", "desktop-ci");
        task.tools = vec!["rust".to_owned()];
        assert!(task.validate("config.toml").is_err(), "MBX is required");
        task.tools = vec![
            "mr-boxington".to_owned(),
            "rust".to_owned(),
            "rust".to_owned(),
        ];
        assert!(task.validate("config.toml").is_err(), "duplicates fail");
        task.tools = vec!["rust".to_owned(), "mr-boxington".to_owned()];
        assert!(
            task.validate("config.toml").is_err(),
            "sort order is required"
        );
        task.tools = vec!["mr-boxington".to_owned(), "rust".to_owned()];
        assert!(task.validate("config.toml").is_ok());
    }

    #[test]
    fn serde_is_stable_closed_and_uses_exact_runner_name() {
        let task = build_task("native-desktop", "desktop-ci");
        let encoded = serde_json::to_string(&task).expect("serialize build task");
        assert_eq!(
            encoded,
            "{\"id\":\"native-desktop\",\"mise_task\":\"desktop-ci\",\"tools\":[\"mr-boxington\",\"rust\"],\"runner\":\"macos-26-arm64\",\"timeout_minutes\":120,\"cargo_build_jobs\":2,\"nextest_test_threads\":2}"
        );
        assert_eq!(
            serde_json::to_string(&task).expect("repeat serialization"),
            encoded
        );
        assert!(serde_json::from_str::<BuildTask>(
            "{\"id\":\"native-desktop\",\"mise_task\":\"desktop-ci\",\"tools\":[\"mr-boxington\",\"rust\"],\"runner\":\"macos-26-arm64\",\"timeout_minutes\":120,\"cargo_build_jobs\":2,\"nextest_test_threads\":2,\"extra\":true}"
        )
        .is_err());
    }
}
