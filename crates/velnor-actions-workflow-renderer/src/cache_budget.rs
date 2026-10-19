//! Generation-time cache budget (invariant I8).
//!
//! The projected hot set must stay at or below 85% of the configured
//! `[workflow] cache_quota_bytes`:
//!
//! - tools: 0.4 GB per distinct written V2 tools identity (the static `d`
//!   input of a job that carries the identity's elected save; a restore
//!   whose identity has no writer, such as the Cache seed's, stores nothing);
//! - sources: two 0.15 GB generations when the sources family exists;
//! - dependency seeds: `seed_max_bytes / 4` per seeded root and family.
//!   GitHub stores the zstd-compressed archive and measured dependency
//!   seeds compress 3.8-4.4x, so a quarter of the raw ceiling bounds one
//!   compressed generation; the janitor keeps one generation per root;
//! - member bundles: 0 until they exist.
//!
//! A generation that cannot fit fails with a clear error instead of
//! letting GitHub evict entries the workflow expects to hit.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
use velnor_actions_contract::{Job, StepKind, StepRole};

use crate::RenderError;

/// Projected bytes of one tools identity.
pub const TOOLS_IDENTITY_BYTES: u64 = 400_000_000;
/// Projected bytes of one sources generation.
pub const SOURCES_GENERATION_BYTES: u64 = 150_000_000;
/// Sources generations the hot set holds (current plus one pending).
pub const SOURCES_GENERATIONS: u64 = 2;
/// Conservative zstd ratio of a dependency seed archive.
pub const SEED_COMPRESSION_RATIO: u64 = 4;

/// Configured quota and seed ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheBudget {
    /// `[workflow] cache_quota_bytes`.
    pub quota_bytes: u64,
    /// `[workflow] seed_max_bytes` (raw).
    pub seed_max_bytes: u64,
}

/// Projected hot-set components of one generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetProjection {
    /// Distinct tools identities.
    pub tools_identities: u64,
    /// The sources family exists.
    pub sources: bool,
    /// Distinct `(family, root)` dependency seeds.
    pub seeds: u64,
}

impl BudgetProjection {
    /// Projected bytes under `budget`.
    #[must_use]
    pub fn bytes(&self, budget: &CacheBudget) -> u64 {
        let sources = if self.sources {
            SOURCES_GENERATIONS * SOURCES_GENERATION_BYTES
        } else {
            0
        };
        self.tools_identities
            .saturating_mul(TOOLS_IDENTITY_BYTES)
            .saturating_add(sources)
            .saturating_add(
                self.seeds
                    .saturating_mul(budget.seed_max_bytes / SEED_COMPRESSION_RATIO),
            )
    }
}

/// Count the hot-set components over every generated workflow's jobs.
#[must_use]
pub fn project(workflows: &[&BTreeMap<String, Job>]) -> BudgetProjection {
    let mut tools = BTreeSet::new();
    let mut seeds = BTreeSet::new();
    let mut sources = false;
    for job in workflows.iter().flat_map(|jobs| jobs.values()) {
        let writes_tools = job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::ToolsCacheSave));
        for step in job.steps.iter().filter(|_| writes_tools) {
            if let StepKind::Action { with, .. } = &step.kind
                && step.role == Some(StepRole::ToolsCacheIdentity)
                && let Some(digest) = with.get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
            {
                tools.insert(digest.clone());
            }
        }
    }
    for step in workflows
        .iter()
        .flat_map(|jobs| jobs.values())
        .flat_map(|job| &job.steps)
    {
        let StepKind::Action { with, .. } = &step.kind else {
            continue;
        };
        let Some(key) = with.get("key") else {
            continue;
        };
        if key.contains("sources_key") {
            sources = true;
        }
        if matches!(
            step.role,
            Some(
                StepRole::DependencySeedLookup
                    | StepRole::DependencySeedRestore
                    | StepRole::DependencySeedSave
            )
        ) && let Some(path) = with.get("path")
        {
            let family = if key.contains("rsdeps_helper") {
                CacheFamily::RsdepsHelper
            } else {
                CacheFamily::Rsdeps
            };
            seeds.insert((family, path.clone()));
        }
    }
    BudgetProjection {
        tools_identities: tools.len() as u64,
        sources,
        seeds: seeds.len() as u64,
    }
}

/// Fail generation when the projected hot set exceeds 85% of the quota.
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] naming every component.
pub fn check(
    workflows: &[&BTreeMap<String, Job>],
    budget: &CacheBudget,
) -> Result<BudgetProjection, RenderError> {
    let projection = project(workflows);
    let projected = projection.bytes(budget);
    let limit = budget.quota_bytes / 100 * 85;
    if projected > limit {
        return Err(RenderError::InvalidWorkflow(format!(
            "cache_budget_exceeded: projected {projected} bytes > 85% of cache_quota_bytes {} ({} tools identities x {TOOLS_IDENTITY_BYTES}, sources {}, {} dependency seeds x seed_max_bytes {}/{SEED_COMPRESSION_RATIO}); lower [workflow] seed_max_bytes or raise cache_quota_bytes",
            budget.quota_bytes,
            projection.tools_identities,
            if projection.sources {
                SOURCES_GENERATIONS * SOURCES_GENERATION_BYTES
            } else {
                0
            },
            projection.seeds,
            budget.seed_max_bytes,
        )));
    }
    Ok(projection)
}

#[cfg(test)]
#[path = "cache_budget_tests.rs"]
mod tests;
