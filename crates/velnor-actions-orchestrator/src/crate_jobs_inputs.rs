//! Shared inputs for crate-job and dependency-seed construction.

use std::collections::BTreeMap;

use velnor_actions_contract::{RustCrateConfig, Step, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;

use crate::discover::Discovery;
#[cfg(test)]
use crate::{OrchestratorError, config::private_wiring::PrivateWiring};
#[cfg(test)]
use velnor_actions_contract::JobTimeout;

/// All authority and rendering inputs for one crate-job build.
#[derive(Clone, Copy)]
pub(crate) struct CrateJobInputs<'a> {
    pub(crate) label: &'a str,
    pub(crate) policy: WorkflowPolicy,
    pub(crate) discovery: &'a Discovery,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) fetch_roots: &'a [String],
    pub(crate) acquire: Option<&'a Step>,
    pub(crate) max_parallel_jobs: u32,
    /// Literal timeout for every emitted crate job.
    pub(crate) timeout: velnor_actions_contract::JobTimeout,
    pub(crate) helper_version: &'a str,
    /// Opt-in cold target initialization; never changes obligation ownership.
    pub(crate) seed_parallel_targets: bool,
    /// Per-package Rust options keyed by Cargo package name.
    pub(crate) crates: &'a BTreeMap<String, RustCrateConfig>,
}

/// [`super::build_crate_jobs`] plus the private-source block (HD-7).
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
#[cfg(test)]
pub(crate) fn build_crate_jobs_with(
    input: CrateJobInputs<'_>,
    private: Option<&PrivateWiring>,
) -> Result<super::CrateBuild, OrchestratorError> {
    super::build_crate_jobs_with_seed_timeout(
        input,
        private,
        JobTimeout::new(
            velnor_actions_contract::config::default_dependency_seed_timeout_minutes(),
        )?,
    )
}
