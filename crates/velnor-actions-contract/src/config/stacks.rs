// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ansible::AnsibleStackConfig;
use super::binary_release::RustBinaryReleaseConfig;
use super::bun::BunStackConfig;
use super::dbt::DbtStackConfig;
use super::gradle::GradleStackConfig;
use super::release::RustReleaseConfig;
use super::rust_inputs::RustInputRule;
use super::rust_suites::RustSuiteConfig;
use super::rust_suites_validate::validate_rust_suites;
use super::tofu::TofuStackConfig;
use crate::discover::Stack;
use crate::errors::ContractError;
use crate::ids::is_component_byte;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StacksConfig {
    #[serde(default)]
    pub ignore: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustStackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu: Option<TofuStackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bun: Option<BunStackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradle: Option<GradleStackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbt: Option<DbtStackConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ansible: Option<AnsibleStackConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredCompileDriver {
    Cargo,

    Mbx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredTestRunner {
    CargoTest,

    CargoNextest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustStackConfig {
    #[serde(default = "default_configurations")]
    pub configurations: Vec<RustConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain_source: Option<super::RustToolchainSource>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_driver: Option<DeclaredCompileDriver>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_runner: Option<DeclaredTestRunner>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<String>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub seed_parallel_targets: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub clippy_all_features: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_empty_test_suite: bool,

    #[serde(default)]
    pub release: RustReleaseConfig,

    #[serde(default)]
    pub binary_release: RustBinaryReleaseConfig,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suites: Vec<RustSuiteConfig>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub crates: BTreeMap<String, RustCrateConfig>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<RustInputRule>,
}

pub const MAX_CRATE_BUILD_JOBS: u32 = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustCrateConfig {
    pub build_jobs: u32,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a borrowed boolean"
)]
fn is_false(value: &bool) -> bool {
    !value
}

fn default_configurations() -> Vec<RustConfiguration> {
    vec![RustConfiguration {
        name: "default".to_owned(),
        features: vec!["default".to_owned()],
        all_features: false,
        target: "host".to_owned(),
    }]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustConfiguration {
    pub name: String,

    #[serde(default)]
    pub features: Vec<String>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub all_features: bool,

    pub target: String,
}

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

#[must_use]
pub fn is_valid_feature_name(feature: &str) -> bool {
    !feature.is_empty()
        && feature.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+' | b'/' | b':' | b'?')
        })
}

impl RustStackConfig {
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: default_configurations(),
            toolchain: None,
            toolchain_source: None,
            compile_driver: None,
            test_runner: None,
            run_ignored: None,
            seed_parallel_targets: false,
            clippy_all_features: false,
            allow_empty_test_suite: false,
            release: RustReleaseConfig::default(),
            binary_release: RustBinaryReleaseConfig::default(),
            suites: Vec::new(),
            crates: BTreeMap::new(),
            inputs: Vec::new(),
        }
    }
}

impl StacksConfig {
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
            let stack = Stack::from_id(id).ok_or_else(|| {
                ContractError::config(file, "stacks.ignore", format!("unknown_stack_id:{id}"))
            })?;
            if !stack.is_ignorable() {
                return Err(ContractError::config(
                    file,
                    "stacks.ignore",
                    format!("stack_not_ignorable:{id}"),
                ));
            }
        }
        if let Some(rust) = &self.rust {
            rust.validate(file)?;
        }
        if let Some(tofu) = &self.tofu {
            tofu.validate(file)?;
        }
        if let Some(bun) = &self.bun {
            bun.validate(file)?;
        }
        if let Some(gradle) = &self.gradle {
            gradle.validate(file)?;
        }
        if let Some(dbt) = &self.dbt {
            dbt.validate(file)?;
        }
        if let Some(ansible) = &self.ansible {
            ansible.validate(file)?;
        }
        Ok(())
    }
}

impl RustStackConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Some(ref tc) = self.toolchain
            && (tc.trim() != tc || tc.is_empty() || !super::tools::is_exact_semver(tc))
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.toolchain",
                format!("invalid_rust_toolchain:{tc}"),
            ));
        }
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
            let default_features = config.features.is_empty() || config.features == ["default"];
            if config.all_features && !default_features {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.features",
                    "features_conflict_all_features",
                ));
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
        self.binary_release.validate(file)?;
        validate_rust_suites(&self.suites, file)?;
        super::rust_inputs::validate_rust_inputs(&self.inputs, file)?;
        self.validate_crates(file)
    }

    fn validate_crates(&self, file: &str) -> Result<(), ContractError> {
        for (package, options) in &self.crates {
            let path = format!("stacks.rust.crates.{package}");
            if package.is_empty()
                || !package
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.crates",
                    format!("bad_package_name:{package}"),
                ));
            }
            if !(1..=MAX_CRATE_BUILD_JOBS).contains(&options.build_jobs) {
                return Err(ContractError::config(
                    file,
                    format!("{path}.build_jobs"),
                    format!("must_be_between_1_and_{MAX_CRATE_BUILD_JOBS}"),
                ));
            }
        }
        Ok(())
    }
}
