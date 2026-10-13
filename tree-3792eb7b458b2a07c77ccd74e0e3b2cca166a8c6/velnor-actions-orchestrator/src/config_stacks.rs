//! `[stacks]` partial configuration: per-stack tables plus load tests.
//!
//! Split from the loader (`config.rs`) under the size gate: stack
//! partials and their materialization live here with the
//! stack-focused load tests.

use serde::Deserialize;
use velnor_actions_contract::config::RustReleaseConfig;
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
    /// Rust release policy; disabled by default.
    release: Option<RustReleaseConfig>,
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
                release: stack.release.unwrap_or_default(),
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
        })
    }
}
#[cfg(test)]
mod tests;
