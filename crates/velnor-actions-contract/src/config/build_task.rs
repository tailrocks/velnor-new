// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::MiseTaskSource;
use super::rust_suites::RustSuiteArtifact;
use crate::errors::ContractError;
use crate::workflow::timeout::JobTimeout;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildTask {
    pub id: String,

    pub mise_task: String,

    pub source: MiseTaskSource,

    pub tools: Vec<String>,

    pub runner: BuildTaskRunner,

    pub timeout_minutes: u16,

    pub cargo_build_jobs: u8,

    pub nextest_test_threads: u8,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RustSuiteArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildTaskRunner {
    #[serde(rename = "macos-26-arm64")]
    Macos26Arm64,

    #[serde(rename = "xcode-27-arm64")]
    Xcode27Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTaskXcodeCapability {
    pub version: &'static str,

    pub build: &'static str,

    pub sdk_version: &'static str,

    pub developer_dir: &'static str,
}

impl BuildTaskRunner {
    #[must_use]
    pub const fn xcode_capability(self) -> BuildTaskXcodeCapability {
        match self {
            Self::Macos26Arm64 => BuildTaskXcodeCapability {
                version: "26.6",
                build: "17F113",
                sdk_version: "26.5",
                developer_dir: "/Applications/Xcode_26.6.app/Contents/Developer",
            },
            Self::Xcode27Arm64 => BuildTaskXcodeCapability {
                version: "27.1",
                build: "27A9275",
                sdk_version: "27.0",
                developer_dir: "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer",
            },
        }
    }

    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::Macos26Arm64 => "macos-26",
            Self::Xcode27Arm64 => "xcode-27",
        }
    }

    #[must_use]
    pub const fn mise_target(self) -> &'static str {
        match self {
            Self::Macos26Arm64 | Self::Xcode27Arm64 => "aarch64-apple-darwin",
        }
    }
}

impl BuildTask {
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
        if !self.source.validate() {
            return Err(ContractError::config(
                file,
                "workflow.tasks.source",
                "bad_build_task_source_or_working_directory",
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
        if !JobTimeout::is_valid_minutes(u32::from(self.timeout_minutes)) {
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
        validate_build_task_artifacts(&self.artifacts, file)
    }
}

fn validate_build_task_artifacts(
    artifacts: &[RustSuiteArtifact],
    file: &str,
) -> Result<(), ContractError> {
    let mut artifact_names = BTreeSet::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        if !artifact_names.insert(artifact.name.as_str()) {
            return Err(ContractError::config(
                file,
                format!("workflow.tasks.artifacts[{index}].name"),
                format!("duplicate_build_task_artifact:{}", artifact.name),
            ));
        }
        super::rust_suites_validate::validate_build_task_artifact(
            artifact,
            file,
            &format!("workflow.tasks.artifacts[{index}]"),
        )?;
    }
    Ok(())
}

#[must_use]
pub fn is_valid_build_tool_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key.as_bytes()[0].is_ascii_alphanumeric()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/'))
}
