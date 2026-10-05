//! Stack selection and per-stack options.
use super::VelnorConfig;
use super::release::RustReleaseConfig;
use super::tofu::TofuStackConfig;
use crate::errors::ContractError;
use crate::ids::is_component_byte;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Stack selection and per-stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StacksConfig {
    /// Sorted, duplicate-free exact registered stack IDs to ignore.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Rust stack options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustStackConfig>,
    /// Tofu stack options; absent means no tofu validation roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu: Option<TofuStackConfig>,
}

/// Declared compile driver (`[stacks.rust] compile_driver`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredCompileDriver {
    /// Plain Cargo compilation.
    Cargo,
    /// MBX compilation.
    Mbx,
}

/// Declared test runner (`[stacks.rust] test_runner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredTestRunner {
    /// Plain `cargo test`.
    CargoTest,
    /// Nextest execution.
    CargoNextest,
}

/// Rust stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustStackConfig {
    /// Rust task configuration variants.
    #[serde(default = "default_configurations")]
    pub configurations: Vec<RustConfiguration>,
    /// Sticky declared compile driver; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_runner: Option<DeclaredTestRunner>,
    /// Ignored test execution mode ("all", "only", "ignored-only", "default").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<String>,
    /// Rust release policy (`[stacks.rust.release]`); disabled by default.
    #[serde(default)]
    pub release: RustReleaseConfig,
    /// Allowlisted Mise custom-task names; only these become `mise run`
    /// steps. Sorted, duplicate-free; empty (the default) emits none.
    #[serde(default)]
    pub custom_tasks: Vec<String>,
}

/// Documented default: one `default` configuration variant list.
fn default_configurations() -> Vec<RustConfiguration> {
    vec![RustConfiguration {
        name: "default".to_owned(),
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
    }]
}

/// One Rust task configuration variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustConfiguration {
    /// Configuration name.
    pub name: String,
    /// Cargo features for this variant.
    #[serde(default)]
    pub features: Vec<String>,
    /// Target triple or `host`.
    pub target: String,
}

/// True for a render-safe Rust target: `host` or a lowercase triple over
/// `[a-z0-9_.+-]` that never starts with `-`.
///
/// Targets flow into `--target` argv and quoted `run:` lines; the
/// allowlist admits nothing the shell or `${{ }}` could evaluate, and
/// flag-shaped values fail closed at config load.
#[must_use]
pub fn is_valid_rust_target(target: &str) -> bool {
    if target == "host" {
        return true;
    }
    !target.is_empty()
        && !target.starts_with('-')
        && target
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'+' | b'-'))
}

/// True for a render-safe Cargo feature name.
///
/// Admits Cargo feature syntax (names plus `dep:`, weak `?`, and `/`
/// qualifiers) and nothing the shell or `${{ }}` could evaluate.
/// Unknown names still fail closed later against declared features.
#[must_use]
pub fn is_valid_feature_name(feature: &str) -> bool {
    !feature.is_empty()
        && feature.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+' | b'/' | b':' | b'?')
        })
}

/// True for a render-safe Mise custom-task name.
///
/// Single source for the config allowlist, the fixed `mise run` argv,
/// and the gate-6 grant: namespaced (`:`) task names plus safe
/// punctuation, never whitespace, separators, or expansions. The first
/// byte must be alphanumeric or `_`: a leading `-` would parse as a
/// `mise run` flag and a leading `.` as a relative path, so both fail
/// closed here before any argv is built.
#[must_use]
pub fn is_valid_custom_task_name(task: &str) -> bool {
    let mut bytes = task.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphanumeric() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

impl RustStackConfig {
    /// Documented default: one `default` configuration, no declarations,
    /// release disabled.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: default_configurations(),
            compile_driver: None,
            test_runner: None,
            run_ignored: None,
            release: RustReleaseConfig::default(),
            custom_tasks: Vec::new(),
        }
    }
}

impl StacksConfig {
    /// Validate ignore list and Rust configurations.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut sorted = self.ignore.clone();
        sorted.sort();
        if sorted != self.ignore {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = self.ignore.iter().map(String::as_str).collect();
        if unique.len() != self.ignore.len() {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "duplicate_stack_id",
            ));
        }
        for id in &self.ignore {
            if !VelnorConfig::REGISTERED_STACKS.contains(&id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.ignore",
                    format!("unknown_stack_id:{id}"),
                ));
            }
        }
        if let Some(rust) = &self.rust {
            rust.validate(file)?;
        }
        if let Some(tofu) = &self.tofu {
            tofu.validate(file)?;
        }
        Ok(())
    }
}

impl RustStackConfig {
    /// Validate Rust configurations (unique nonempty names).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.configurations.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.configurations",
                "empty_configurations",
            ));
        }
        let mut names = BTreeSet::new();
        for config in &self.configurations {
            if config.name.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.name",
                    "empty_name",
                ));
            }
            if !config.name.bytes().all(is_component_byte) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.name",
                    format!("bad_component:{}", config.name),
                ));
            }
            if !names.insert(config.name.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations",
                    format!("duplicate_configuration:{}", config.name),
                ));
            }
            if !is_valid_rust_target(&config.target) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.target",
                    format!("bad_target:{}", config.target),
                ));
            }
            for feature in &config.features {
                if !is_valid_feature_name(feature) {
                    return Err(ContractError::config(
                        file,
                        "stacks.rust.configurations.features",
                        format!("bad_feature:{feature}"),
                    ));
                }
            }
        }
        if let Some(mode) = &self.run_ignored
            && !matches!(mode.as_str(), "all" | "only" | "ignored-only" | "default")
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.run_ignored",
                format!("bad_run_ignored:{mode}"),
            ));
        }
        self.release.validate(file)?;
        self.validate_custom_tasks(file)?;
        Ok(())
    }

    /// Validate the custom-task allowlist: sorted, unique, safe names.
    ///
    /// The name rule is [`is_valid_custom_task_name`], shared with the
    /// `mise run` argv builder and the qualified-task paths.
    /// # Errors
    fn validate_custom_tasks(&self, file: &str) -> Result<(), ContractError> {
        let mut sorted = self.custom_tasks.clone();
        sorted.sort();
        if sorted != self.custom_tasks {
            return Err(ContractError::config(
                file,
                "stacks.rust.custom_tasks",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = self.custom_tasks.iter().map(String::as_str).collect();
        if unique.len() != self.custom_tasks.len() {
            return Err(ContractError::config(
                file,
                "stacks.rust.custom_tasks",
                "duplicate_custom_task",
            ));
        }
        for task in &self.custom_tasks {
            if !is_valid_custom_task_name(task) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.custom_tasks",
                    format!("bad_custom_task:{task}"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RustConfiguration, is_valid_custom_task_name, is_valid_rust_target};
    use crate::config::RustStackConfig;

    #[test]
    fn target_grammar_accepts_host_and_triples_only() {
        for target in ["host", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            assert!(is_valid_rust_target(target), "{target}");
        }
        // Proven X2 PoC plus metacharacter and flag-shaped values.
        for target in [
            "",
            " ",
            "${{ secrets.CARGO_REGISTRY_TOKEN }}",
            "$TRIPLE",
            "`id`",
            "-foo",
            "HOST",
            "x86_64 unknown",
            "a/b",
        ] {
            assert!(!is_valid_rust_target(target), "{target:?}");
        }
    }

    #[test]
    fn task_name_grammar_matches_mise_tasks() {
        for task in ["audit", "build:all", "a-b_c.d:e", "Test123"] {
            assert!(is_valid_custom_task_name(task), "{task}");
        }
        for task in ["", "a b", "a/b", "${{ x }}", "$t", "`t`", "a\nb"] {
            assert!(!is_valid_custom_task_name(task), "{task:?}");
        }
    }

    #[test]
    fn hostile_target_and_task_fail_validation() {
        let mut stack = RustStackConfig::default_config();
        stack.configurations = vec![RustConfiguration {
            name: "default".to_owned(),
            features: Vec::new(),
            target: "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        }];
        let err = stack.validate("config.toml").expect_err("PoC target fails");
        assert!(err.to_string().contains("bad_target"), "{err}");
        let mut stack = RustStackConfig::default_config();
        stack.custom_tasks = vec!["audit".to_owned(), "evil task".to_owned()];
        let err = stack.validate("config.toml").expect_err("bad task fails");
        assert!(err.to_string().contains("bad_custom_task"), "{err}");
    }
}
