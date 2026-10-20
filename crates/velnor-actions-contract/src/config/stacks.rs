//! Stack selection and per-stack options.
use super::binary_release::RustBinaryReleaseConfig;
use super::bun::BunStackConfig;
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
    /// Bun stack options; absent means no Bun workspaces (hard decision 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bun: Option<BunStackConfig>,
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
    /// Optional exact `major.minor.patch` Rust toolchain override (`[stacks.rust] toolchain`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,
    /// Sticky declared compile driver; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_runner: Option<DeclaredTestRunner>,
    /// Ignored test execution mode ("all", "only", "ignored-only", "default").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<String>,
    /// Initialize missing parallel check targets from the completed test build.
    /// Existing target lanes are preserved; disabled by default.
    #[serde(default, skip_serializing_if = "is_false")]
    pub seed_parallel_targets: bool,
    /// Activate all package features for Clippy only; this does not change test or documentation tasks.
    #[serde(default, skip_serializing_if = "is_false")]
    pub clippy_all_features: bool,
    /// Rust release policy (`[stacks.rust.release]`); disabled by default.
    #[serde(default)]
    pub release: RustReleaseConfig,
    /// Single-package binary release policy; disabled by default.
    #[serde(default)]
    pub binary_release: RustBinaryReleaseConfig,
    /// Named Nextest suites (`[[stacks.rust.suites]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suites: Vec<RustSuiteConfig>,
    /// Per-package options (`[stacks.rust.crates.<package>]`), keyed by Cargo package name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub crates: BTreeMap<String, RustCrateConfig>,
    /// Declared inputs outside each package's own directory
    /// (`[[stacks.rust.inputs]]`), consumed by affected-work selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<RustInputRule>,
}

/// Largest accepted per-crate `build_jobs`.
pub const MAX_CRATE_BUILD_JOBS: u32 = 64;

/// Per-package Rust options (`[stacks.rust.crates.<package>]`).
///
/// `build_jobs` caps Cargo's parallel compiler processes
/// (`CARGO_BUILD_JOBS`) for every Cargo step of that package's crate job:
/// the test-executable build and each parallel check. It bounds peak memory
/// for crates whose rustc processes are large (a lib and its unit-test
/// harness compiled together can need several GiB each). It never exceeds
/// `resources.compiler_process_budget`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustCrateConfig {
    /// Cargo compiler process cap for this package's crate job.
    pub build_jobs: u32,
}

/// Documented default: one `default` configuration variant list.
fn default_configurations() -> Vec<RustConfiguration> {
    vec![RustConfiguration {
        name: "default".to_owned(),
        features: vec!["default".to_owned()],
        all_features: false,
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
    /// Activate every feature declared by each package in this variant (`--all-features`).
    /// This conflicts with a feature list other than empty or `default` only.
    #[serde(default, skip_serializing_if = "is_false")]
    pub all_features: bool,
    /// Target triple or `host`.
    pub target: String,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if needs a reference predicate"
)]
const fn is_false(value: &bool) -> bool {
    !*value
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

impl RustStackConfig {
    /// Documented default: one `default` configuration, no declarations,
    /// release disabled.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: default_configurations(),
            toolchain: None,
            compile_driver: None,
            test_runner: None,
            run_ignored: None,
            seed_parallel_targets: false,
            clippy_all_features: false,
            release: RustReleaseConfig::default(),
            binary_release: RustBinaryReleaseConfig::default(),
            suites: Vec::new(),
            crates: BTreeMap::new(),
            inputs: Vec::new(),
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
        Ok(())
    }
}

impl RustStackConfig {
    /// Validate Rust configurations (unique nonempty names) and declared suites.
    /// # Errors
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

    /// Validate per-package options: package-name grammar and `build_jobs` range.
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

#[cfg(test)]
#[path = "stacks_tests.rs"]
mod tests;
