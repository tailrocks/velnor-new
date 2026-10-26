// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{env, path::Path, time::Duration};

use crate::OrchestratorError;

pub(crate) const TARGET_ROOT_ENV: &str = "VELNOR_TARGET_ROOT";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ParallelTargetLane {
    Clippy,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SeedOutcome {
    Cold,
    Seeded {
        lanes: Vec<String>,
        bytes_per_lane: u64,
    },
    Skipped {
        reason: &'static str,
    },
}

pub(crate) fn seed_parallel_targets_from_env() -> Result<SeedOutcome, OrchestratorError> {
    let workspace = env::var_os("GITHUB_WORKSPACE").ok_or_else(|| invalid("workspace_missing"))?;
    let target = env::var(TARGET_ROOT_ENV).map_err(|_| invalid("target_root_missing"))?;
    let lane_text = env::var(TARGET_LANES_ENV).map_err(|_| invalid("target_lanes_missing"))?;
    let lanes = parse_lanes(&lane_text)?;
    seed_parallel_targets(Path::new(&workspace), &target, &lanes)
}

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
