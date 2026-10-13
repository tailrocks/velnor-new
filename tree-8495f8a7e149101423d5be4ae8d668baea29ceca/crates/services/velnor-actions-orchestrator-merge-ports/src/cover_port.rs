//! Cover behavior consumed by merge, behind a port trait.
//!
//! Merge calls exactly these five cover functions. The hub implements
//! [`CoverPort`] by delegating to its cover modules, so merge depends
//! only on this contract and the direct cover/merge cycle is broken.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{MatrixEntry, MatrixReport, Plan};
use velnor_actions_orchestrator_core::OrchestratorError;

use super::cover_types::{CoverSinks, Partition, Signals};
use super::merge_types::{BaselineManifest, MergeRequest};
use super::shard_types::{ResourceLimits, ShardProof};

/// Cover checks 2-3 plus revalidation, shard proofs, and budgets.
pub trait CoverPort {
    /// Check 2: keep the first valid report per expected ID exactly.
    fn partition_reports<'a>(
        &self,
        request: &'a MergeRequest,
        entries: &BTreeMap<&str, &MatrixEntry>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    ) -> Partition<'a>;

    /// Check 3 for one entry: binding, task set, counts, digests; fold on cover.
    fn cover_entry(
        &self,
        request: &MergeRequest,
        entry: &MatrixEntry,
        report: &MatrixReport,
        obligations: &BTreeMap<&str, &str>,
        sinks: &mut CoverSinks<'_>,
    ) -> Result<bool, OrchestratorError>;

    /// Revalidate planner coverage against the trusted baseline manifest.
    fn revalidate_coverage(
        &self,
        plan: &Plan,
        manifest: Option<&BaselineManifest>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    );

    /// Validate merge proofs for the sharded bases of one entry.
    fn check_entry_shards(
        &self,
        bases: &BTreeSet<String>,
        empty_count: u32,
        proofs: &[ShardProof],
        obligations: &BTreeMap<String, String>,
    ) -> Result<(), String>;

    /// Reject zero budgets, over-budget shards, and above-capacity concurrency.
    fn validate_budgets(&self, limits: &ResourceLimits) -> Result<(), String>;
}
