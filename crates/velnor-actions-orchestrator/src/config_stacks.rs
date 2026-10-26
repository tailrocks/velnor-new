// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_actions_contract::config::{
    AnsibleStackConfig, BunStackConfig, DbtStackConfig, RustBinaryReleaseConfig, RustReleaseConfig,
};
use velnor_actions_contract::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    TofuStackConfig, Utf8RepoRelDir,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialStacks {
    #[serde(default)]
    ignore: Vec<String>,
    rust: Option<PartialRustStack>,
    tofu: Option<PartialTofuStack>,
    bun: Option<BunStackConfig>,
    gradle: Option<velnor_actions_contract::config::gradle::GradleStackConfig>,
    dbt: Option<DbtStackConfig>,
    ansible: Option<AnsibleStackConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialRustStack {
    configurations: Option<Vec<RustConfiguration>>,
    compile_driver: Option<DeclaredCompileDriver>,
    test_runner: Option<DeclaredTestRunner>,
    run_ignored: Option<String>,
    seed_parallel_targets: Option<bool>,
    clippy_all_features: Option<bool>,
    release: Option<RustReleaseConfig>,
    binary_release: Option<RustBinaryReleaseConfig>,
    #[serde(default)]
    suites: Vec<velnor_actions_contract::RustSuiteConfig>,
    #[serde(default)]
    toolchain: Option<String>,
    #[serde(default)]
    toolchain_source: Option<velnor_actions_contract::config::RustToolchainSource>,
    #[serde(default)]
    crates: std::collections::BTreeMap<String, velnor_actions_contract::RustCrateConfig>,
    #[serde(default)]
    inputs: Vec<velnor_actions_contract::RustInputRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartialTofuStack {
    roots: Option<Vec<String>>,
}

impl PartialStacks {
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
                toolchain_source: stack.toolchain_source,
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
            gradle: self.gradle,
            dbt: self.dbt,
            ansible: self.ansible,
        })
    }
}
