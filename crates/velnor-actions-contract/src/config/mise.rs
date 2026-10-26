// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::HostContainerProfile;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[path = "check_system_tools.rs"]
mod check_system_tools;
pub use check_system_tools::{
    CheckSystemTool, CheckSystemToolKind, is_valid_check_system_build,
    is_valid_check_system_version,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseCheck {
    pub id: String,

    pub task: String,

    #[serde(default = "default_directory")]
    pub directory: String,

    pub runner: CheckRunner,

    pub inputs: Vec<String>,

    pub tools: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_tools: Vec<CheckSystemTool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<CheckEvidence>,

    #[serde(default = "default_timeout")]
    pub timeout_minutes: u32,
}

fn default_directory() -> String {
    ".".to_owned()
}
fn default_timeout() -> u32 {
    u32::from(crate::workflow::timeout::JobTimeout::DEFAULT.minutes())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRunner {
    pub label: String,

    pub platform: CheckPlatform,

    pub executor: CheckExecutor,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<HostContainerProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckPlatform {
    LinuxX64,

    MacosArm64,

    MacosX64,
}

impl CheckPlatform {
    #[must_use]
    pub const fn release_target(self) -> crate::ReleaseTarget {
        match self {
            Self::LinuxX64 => crate::ReleaseTarget::LinuxX86_64,
            Self::MacosArm64 => crate::ReleaseTarget::MacosArm64,
            Self::MacosX64 => crate::ReleaseTarget::MacosX86_64,
        }
    }

    #[must_use]
    pub const fn target(self) -> &'static str {
        self.release_target().triple()
    }

    #[must_use]
    pub const fn os(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux",
            Self::MacosArm64 | Self::MacosX64 => "macos",
        }
    }

    #[must_use]
    pub const fn arch(self) -> &'static str {
        match self {
            Self::MacosArm64 => "aarch64",
            Self::LinuxX64 | Self::MacosX64 => "x86_64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckExecutor {
    Hosted,

    EphemeralSelfHosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub path: String,

    pub expected_scenarios: Vec<String>,
}

#[must_use]
pub fn is_valid_mise_task_name(task: &str) -> bool {
    let mut bytes = task.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphanumeric() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn safe_component(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphanumeric() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn safe_path(value: &str, root_allowed: bool) -> bool {
    (root_allowed && value == ".")
        || (!value.is_empty()
            && value.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            }))
}

fn unique_values(values: &[String], valid: fn(&str) -> bool) -> bool {
    let unique: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    unique.len() == values.len() && values.iter().all(|value| valid(value))
}

impl MiseCheck {
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let bad = |field: &str, problem: &str| {
            ContractError::config(file, format!("{key}.{field}"), problem)
        };
        if !safe_component(&self.id) || self.id.len() > 128 {
            return Err(bad("id", "bad_check_id"));
        }
        if !is_valid_mise_task_name(&self.task) || self.task.len() > 128 {
            return Err(bad("task", "bad_mise_task"));
        }
        if !safe_path(&self.directory, true) || self.directory.len() > 1024 {
            return Err(bad("directory", "bad_check_directory"));
        }
        if self.inputs.is_empty() || !unique_values(&self.inputs, |value| safe_path(value, false)) {
            return Err(bad("inputs", "invalid_check_inputs"));
        }
        if self.tools.len() > 128 || !unique_values(&self.tools, safe_component) {
            return Err(bad("tools", "invalid_check_tools"));
        }
        if !crate::workflow::timeout::JobTimeout::is_valid_minutes(self.timeout_minutes) {
            return Err(bad("timeout_minutes", "invalid_check_timeout"));
        }
        self.runner.validate(file, &format!("{key}.runner"))?;
        check_system_tools::validate_system_tools(
            &self.system_tools,
            self.runner.platform,
            file,
            key,
        )?;
        if let Some(evidence) = &self.evidence {
            if !safe_path(&evidence.path, false)
                || evidence.path.len() > 1024
                || std::path::Path::new(&evidence.path).extension()
                    != Some(std::ffi::OsStr::new("json"))
            {
                return Err(bad("evidence.path", "bad_evidence_path"));
            }
            if evidence.expected_scenarios.is_empty()
                || evidence.expected_scenarios.len() > 64
                || evidence
                    .expected_scenarios
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || evidence.expected_scenarios.iter().any(|id| id.len() > 128)
                || !unique_values(&evidence.expected_scenarios, is_valid_mise_task_name)
            {
                return Err(bad(
                    "evidence.expected_scenarios",
                    "invalid_expected_scenarios",
                ));
            }
        }
        Ok(())
    }
}

impl CheckRunner {
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let bad = |field: &str, problem: &str| {
            ContractError::config(file, format!("{key}.{field}"), problem)
        };
        match self.executor {
            CheckExecutor::Hosted
                if crate::targets::ReleaseTarget::for_runner_label(&self.label)
                    .map(crate::targets::ReleaseTarget::triple)
                    != Some(self.platform.target()) =>
            {
                return Err(bad("label", "hosted_runner_platform_mismatch"));
            }
            CheckExecutor::EphemeralSelfHosted
                if !safe_component(&self.label)
                    || self.label == "self-hosted"
                    || self.label == "latest"
                    || self.label.ends_with("-latest")
                    || crate::targets::ReleaseTarget::for_runner_label(&self.label).is_some() =>
            {
                return Err(bad("label", "bad_ephemeral_runner_label"));
            }
            _ => {}
        }
        if let Some(container) = &self.container {
            container.validate(
                self.platform,
                self.executor,
                file,
                &format!("{key}.container"),
            )?;
        }
        Ok(())
    }
}
