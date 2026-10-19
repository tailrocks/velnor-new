//! Per-crate Cargo compile parallelism cap (`[stacks.rust.crates.<package>] build_jobs`).

use std::collections::BTreeMap;

use velnor_actions_contract::{RustCrateConfig, Step, StepKind};

use super::is_rust;
use crate::OrchestratorError;
use crate::discover::Discovery;

/// Fail closed when `[stacks.rust.crates]` names a package no task belongs to.
///
/// A typo would otherwise leave the memory cap silently unapplied.
pub(super) fn reject_unknown_crates(
    crates: &BTreeMap<String, RustCrateConfig>,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    for package in crates.keys() {
        if !discovery
            .proposals
            .iter()
            .any(|task| task.display_name == *package && is_rust(task))
        {
            return Err(OrchestratorError::config(
                crate::config::CONFIG_REL,
                format!("stacks.rust.crates.{package}"),
                "unknown_rust_package",
            ));
        }
    }
    Ok(())
}

/// Environment variable Cargo reads for its compiler process cap.
const CARGO_BUILD_JOBS_ENV: &str = "CARGO_BUILD_JOBS";

/// Limit Cargo compiler processes for each Rust task step.
///
/// Apply the limit to the test build and each parallel check.
/// This includes Clippy, documentation, tests, and doctests.
/// Each task has its own compiler process limit.
pub(super) fn cap_cargo_jobs(step: &mut Step, build_jobs: u32) {
    if let StepKind::TaskExecution { env, .. } = &mut step.kind {
        env.insert(CARGO_BUILD_JOBS_ENV.to_owned(), build_jobs.to_string());
    }
}
