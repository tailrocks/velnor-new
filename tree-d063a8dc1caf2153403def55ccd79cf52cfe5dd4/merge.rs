//! Merge entrypoint over the extracted merge crate (cover/merge seam).
//!
//! Merge behavior lives in `velnor-actions-orchestrator-merge` and calls
//! cover only through [`CoverPort`]; this module implements the port over
//! the hub's cover modules and keeps the original `merge_internal`
//! entrypoint byte-identical for the CLI and integration tests.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{MatrixEntry, MatrixReport, Plan};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_merge::merge_internal_with;
use velnor_actions_orchestrator_merge_ports::{
    CoverPort, CoverSinks, Partition, ResourceLimits, ShardProof, Signals,
};

pub(crate) use velnor_actions_orchestrator_merge::{
    BaselineManifest, MergeRequest, required_evidence,
};

/// Hub cover behavior behind the merge port.
struct HubCover;

impl CoverPort for HubCover {
    fn partition_reports<'a>(
        &self,
        request: &'a MergeRequest,
        entries: &BTreeMap<&str, &MatrixEntry>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    ) -> Partition<'a> {
        velnor_actions_orchestrator_cover::cover::partition_reports(
            request,
            entries,
            signals,
            miss_reasons,
        )
    }

    fn cover_entry(
        &self,
        request: &MergeRequest,
        entry: &MatrixEntry,
        report: &MatrixReport,
        obligations: &BTreeMap<&str, &str>,
        sinks: &mut CoverSinks<'_>,
    ) -> Result<bool, OrchestratorError> {
        velnor_actions_orchestrator_cover::cover::cover_entry(
            request,
            entry,
            report,
            obligations,
            sinks,
        )
    }

    fn revalidate_coverage(
        &self,
        plan: &Plan,
        manifest: Option<&BaselineManifest>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    ) {
        velnor_actions_orchestrator_cover::cover::revalidate_coverage(
            plan,
            manifest,
            signals,
            miss_reasons,
        );
    }

    fn check_entry_shards(
        &self,
        bases: &BTreeSet<String>,
        empty_count: u32,
        proofs: &[ShardProof],
        obligations: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        velnor_actions_orchestrator_cover::cover::shard::check_entry_shards(
            bases,
            empty_count,
            proofs,
            obligations,
        )
    }

    fn validate_budgets(&self, limits: &ResourceLimits) -> Result<(), String> {
        velnor_actions_orchestrator_cover::cover::shard::validate_budgets(limits)
    }
}

/// Aggregate matrix reports into the final gate report (schema-1 JSON).
///
/// Only the request envelope (JSON shape, schema, run key) fails
/// outright; every evidence failure yields a diagnostic `planning_failed`
/// verdict with closed failure tokens. A missing plan still yields a
/// verdict instead of an error. Consumes the emitted plan only and never
/// rediscovers repository state.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// response-encoding failures.
pub fn merge_internal(request_json: &str) -> Result<String, OrchestratorError> {
    merge_internal_with(&HubCover, request_json)
}
