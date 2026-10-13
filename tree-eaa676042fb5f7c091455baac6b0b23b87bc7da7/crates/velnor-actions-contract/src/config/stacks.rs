//! Stack selection and per-stack options.
use super::VelnorConfig;
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

impl RustStackConfig {
    /// Documented default: one `default` configuration, no declarations.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: default_configurations(),
            compile_driver: None,
            test_runner: None,
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
            if config.target.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.target",
                    "empty_target",
                ));
            }
        }
        Ok(())
    }
}
