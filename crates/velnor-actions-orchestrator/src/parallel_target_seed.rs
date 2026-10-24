//! Bounded, independent initialization of cold same-job Cargo check targets.
//!
//! This is an optional optimization. It copies only the completed `target/debug`
//! baseline into absent, fixed Cargo lanes and admits each lane atomically.

use std::{env, path::Path, time::Duration};

use crate::OrchestratorError;

/// Environment key carrying the workspace-relative `target` directory.
pub(crate) const TARGET_ROOT_ENV: &str = "VELNOR_TARGET_ROOT";
/// Environment key carrying sorted lane identifiers separated by commas.
pub(crate) const TARGET_LANES_ENV: &str = "VELNOR_TARGET_LANES";

const MAX_SECONDS: u64 = 20;
const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 64_000;
const RESERVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const READ_CHUNK: usize = 1024 * 1024;

#[derive(Clone, Copy)]
struct SeedLimits {
    seconds: Duration,
    bytes: u64,
    entries: usize,
    reserve: u64,
}

const DEFAULT_LIMITS: SeedLimits = SeedLimits {
    seconds: Duration::from_secs(MAX_SECONDS),
    bytes: MAX_BYTES,
    entries: MAX_ENTRIES,
    reserve: RESERVE_BYTES,
};

/// One fixed concurrent Cargo output lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ParallelTargetLane {
    /// Clippy output lane.
    Clippy,
    /// Rust documentation output lane.
    Doc,
}

impl ParallelTargetLane {
    fn as_str(self) -> &'static str {
        match self {
            Self::Clippy => "velnor-clippy",
            Self::Doc => "velnor-doc",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "velnor-clippy" => Some(Self::Clippy),
            "velnor-doc" => Some(Self::Doc),
            _ => None,
        }
    }
}

/// Result of the optional seed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SeedOutcome {
    /// No cold lanes were requested or the baseline was absent.
    Cold,
    /// Each listed lane was copied and admitted independently.
    Seeded {
        /// The lanes admitted by atomic rename.
        lanes: Vec<String>,
        /// The baseline byte count copied into each lane.
        bytes_per_lane: u64,
    },
    /// Safety budget or host capability prevented the optional copy.
    Skipped {
        /// Why optional seeding was skipped.
        reason: &'static str,
    },
}

/// Parse the closed environment request and seed lanes under `GITHUB_WORKSPACE`.
///
/// # Errors
/// Returns an error for missing, malformed, or unsafe input, or for filesystem
/// changes that do not match the admitted source/staging identities.
pub(crate) fn seed_parallel_targets_from_env() -> Result<SeedOutcome, OrchestratorError> {
    let workspace = env::var_os("GITHUB_WORKSPACE").ok_or_else(|| invalid("workspace_missing"))?;
    let target = env::var(TARGET_ROOT_ENV).map_err(|_| invalid("target_root_missing"))?;
    let lane_text = env::var(TARGET_LANES_ENV).map_err(|_| invalid("target_lanes_missing"))?;
    let lanes = parse_lanes(&lane_text)?;
    seed_parallel_targets(Path::new(&workspace), &target, &lanes)
}

/// Seed missing, fixed Cargo target lanes from the current target's debug tree.
///
/// The workspace and target are opened with no-follow directory descriptors.
/// Existing lanes are preserved. Every new lane is copied to a private staging
/// directory, checked against the captured source inventory, and installed by
/// an atomic no-replace rename.
///
/// # Errors
/// Returns an error for malformed paths/lanes, unsafe filesystem entries,
/// concurrent source changes, or staging ownership changes. Budget exhaustion,
/// capacity exhaustion, and unsupported atomic rename are non-fatal skips.
pub(crate) fn seed_parallel_targets(
    workspace: &Path,
    target_root: &str,
    lanes: &[ParallelTargetLane],
) -> Result<SeedOutcome, OrchestratorError> {
    seed_parallel_targets_with_limits(workspace, target_root, lanes, DEFAULT_LIMITS)
}

#[path = "parallel_target_seed_execution.rs"]
mod execution;
#[path = "parallel_target_seed_filesystem.rs"]
mod filesystem;
#[path = "parallel_target_seed_inventory.rs"]
mod inventory;
use execution::seed_parallel_targets_with_limits;
use filesystem::{invalid, parse_lanes};

#[cfg(test)]
#[path = "parallel_target_seed_tests.rs"]
mod tests;
