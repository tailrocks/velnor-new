//! Explicit repository-owned Mise checks; runner identity stays typed.
use super::HostContainerProfile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use velnor_actions_contract::errors::ContractError;
mod check_system_tools;
pub use check_system_tools::{CheckSystemTool, CheckSystemToolKind};

/// One opaque check, always executed; no cached-result reuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseCheck {
    /// Stable check identity.
    pub id: String,
    /// Repository-owned Mise task name; never arbitrary argv.
    pub task: String,
    /// Repository-relative task directory.
    #[serde(default = "default_directory")]
    pub directory: String,
    /// Explicit execution placement.
    pub runner: CheckRunner,
    /// Declared repository-relative input files.
    pub inputs: Vec<String>,
    /// Declared tools from repository Mise configuration.
    pub tools: Vec<String>,
    /// Exact pins for native tools already installed on the runner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_tools: Vec<CheckSystemTool>,
    /// Optional structured scenario evidence produced by the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<CheckEvidence>,
    /// Bounded job execution time.
    #[serde(default = "default_timeout")]
    pub timeout_minutes: u32,
}

fn default_directory() -> String {
    ".".to_owned()
}
const fn default_timeout() -> u32 {
    30
}

/// Runner label, explicit platform, and execution ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRunner {
    /// Exact hosted catalog label or custom ephemeral scale-set label.
    pub label: String,
    /// Target identity, never inferred from arbitrary runner labels.
    pub platform: CheckPlatform,
    /// Hosted or repository-provisioned ephemeral placement.
    pub executor: CheckExecutor,
    /// Fully qualified installed host container runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<HostContainerProfile>,
}

/// Supported binary platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckPlatform {
    /// Linux on x86-64.
    LinuxX64,
    /// macOS on Apple Silicon.
    MacosArm64,
    /// macOS on Intel.
    MacosX64,
}

impl CheckPlatform {
    /// Canonical release target for this declared runner platform.
    #[must_use]
    pub const fn release_target(self) -> velnor_actions_contract_release::ReleaseTarget {
        match self {
            Self::LinuxX64 => velnor_actions_contract_release::ReleaseTarget::LinuxX86_64,
            Self::MacosArm64 => velnor_actions_contract_release::ReleaseTarget::MacosArm64,
            Self::MacosX64 => velnor_actions_contract_release::ReleaseTarget::MacosX86_64,
        }
    }

    /// Exact supported release target.
    #[must_use]
    pub const fn target(self) -> &'static str {
        self.release_target().triple()
    }
    /// Host operating system expected by runtime validation.
    #[must_use]
    pub const fn os(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux",
            Self::MacosArm64 | Self::MacosX64 => "macos",
        }
    }
    /// Host architecture expected by runtime validation.
    #[must_use]
    pub const fn arch(self) -> &'static str {
        match self {
            Self::MacosArm64 => "aarch64",
            Self::LinuxX64 | Self::MacosX64 => "x86_64",
        }
    }
}

/// Runner execution policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckExecutor {
    /// Catalogued GitHub-hosted runner.
    Hosted,
    /// Custom single-label ephemeral runner or scale set.
    EphemeralSelfHosted,
}

/// Required named scenario evidence; an empty report cannot prove tests ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    /// Repository-relative JSON report path.
    pub path: String,
    /// Nonempty unique expected executed scenario IDs.
    pub expected_scenarios: Vec<String>,
}

/// Safe namespaced Mise task name; flags, paths and expansions rejected.
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
    /// Validate the closed configuration contract.
    /// # Errors
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
        if !(1..=360).contains(&self.timeout_minutes) {
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
    /// Validate placement and capability constraints.
    /// # Errors
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        let bad = |field: &str, problem: &str| {
            ContractError::config(file, format!("{key}.{field}"), problem)
        };
        match self.executor {
            CheckExecutor::Hosted
                if velnor_actions_contract_release::targets::ReleaseTarget::for_runner_label(&self.label)
                    .map(velnor_actions_contract_release::targets::ReleaseTarget::triple)
                    != Some(self.platform.target()) =>
            {
                return Err(bad("label", "hosted_runner_platform_mismatch"));
            }
            CheckExecutor::EphemeralSelfHosted
                if !safe_component(&self.label)
                    || self.label == "self-hosted"
                    || self.label == "latest"
                    || self.label.ends_with("-latest")
                    || velnor_actions_contract_release::targets::ReleaseTarget::for_runner_label(&self.label).is_some() =>
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

#[cfg(test)]
mod tests;
