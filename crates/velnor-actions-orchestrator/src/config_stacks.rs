//! `[stacks]` partial configuration: per-stack tables plus load tests.
//!
//! Split from the loader (`config.rs`) under the size gate: stack
//! partials and their materialization live here with the
//! stack-focused load tests.

use serde::Deserialize;
use velnor_actions_contract::config::{
    BunStackConfig, DbtStackConfig, RustBinaryReleaseConfig, RustReleaseConfig,
};
use velnor_actions_contract::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    TofuStackConfig, Utf8RepoRelDir,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;

/// Stacks section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialStacks {
    /// Stack IDs to ignore.
    #[serde(default)]
    ignore: Vec<String>,
    /// Rust stack options.
    rust: Option<PartialRustStack>,
    /// Tofu stack options.
    tofu: Option<PartialTofuStack>,
    /// Bun stack options (strict: `roots` is required).
    bun: Option<BunStackConfig>,
    /// dbt stack options (strict: `projects` is required).
    dbt: Option<DbtStackConfig>,
}

/// Rust stack section with every value optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialRustStack {
    /// Rust task configuration variants.
    configurations: Option<Vec<RustConfiguration>>,
    /// Sticky declared compile driver.
    compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner.
    test_runner: Option<DeclaredTestRunner>,
    /// Ignored test execution mode.
    run_ignored: Option<String>,
    /// Optional cold parallel-target initialization after the test build.
    seed_parallel_targets: Option<bool>,
    /// Activate all package features for Clippy only; disabled by default.
    clippy_all_features: Option<bool>,
    /// Rust release policy; disabled by default.
    release: Option<RustReleaseConfig>,
    /// Single-package binary release policy; disabled by default.
    binary_release: Option<RustBinaryReleaseConfig>,
    /// Named Nextest suites (`[[stacks.rust.suites]]`).
    #[serde(default)]
    suites: Vec<velnor_actions_contract::RustSuiteConfig>,
    /// Optional exact `X.Y.Z` Rust toolchain channel override.
    #[serde(default)]
    toolchain: Option<String>,
    /// Per-package options (`[stacks.rust.crates.<package>]`).
    #[serde(default)]
    crates: std::collections::BTreeMap<String, velnor_actions_contract::RustCrateConfig>,
    /// Declared inputs outside package directories (`[[stacks.rust.inputs]]`).
    #[serde(default)]
    inputs: Vec<velnor_actions_contract::RustInputRule>,
}

/// Tofu stack section: `roots` required when the table is present.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialTofuStack {
    /// Tofu validation roots; `.` names the repository root.
    roots: Option<Vec<String>>,
}

impl PartialStacks {
    /// Fill stacks defaults; a tofu table requires `roots`.
    pub(crate) fn materialize(self) -> Result<StacksConfig, OrchestratorError> {
        let rust = self.rust.map(|stack| {
            let defaults = RustStackConfig::default_config();
            RustStackConfig {
                configurations: stack.configurations.unwrap_or(defaults.configurations),
                compile_driver: stack.compile_driver,
                test_runner: stack.test_runner,
                run_ignored: stack.run_ignored,
                seed_parallel_targets: stack.seed_parallel_targets.unwrap_or(false),
                clippy_all_features: stack.clippy_all_features.unwrap_or(false),
                release: stack.release.unwrap_or_default(),
                binary_release: stack.binary_release.unwrap_or_default(),
                suites: stack.suites,
                toolchain: stack.toolchain,
                crates: stack.crates,
                inputs: stack.inputs,
            }
        });
        let tofu = self
            .tofu
            .map(|stack| {
                let roots = stack.roots.ok_or_else(|| {
                    OrchestratorError::config(
                        CONFIG_REL,
                        "stacks.tofu.roots",
                        "missing_required_roots",
                    )
                })?;
                Ok::<TofuStackConfig, OrchestratorError>(TofuStackConfig {
                    roots: roots.into_iter().map(Utf8RepoRelDir::from_raw).collect(),
                })
            })
            .transpose()?;
        Ok(StacksConfig {
            ignore: self.ignore,
            rust,
            tofu,
            bun: self.bun,
            dbt: self.dbt,
        })
    }
}

#[cfg(test)]
#[path = "config_stacks_tests.rs"]
mod tests;
