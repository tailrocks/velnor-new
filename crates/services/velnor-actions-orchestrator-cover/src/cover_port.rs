//! Cover behavior behind the merge port.
//!
//! Merge calls cover only through [`CoverPort`]; this module is the
//! production implementation over the [`cover`](super::cover) behavior.
//! Entry points needing the merge driver pass `&Cover` explicitly.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{MatrixEntry, MatrixReport, Plan};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_merge_ports::{
    BaselineManifest, CoverPort, CoverSinks, MergeRequest, Partition, ResourceLimits, ShardProof,
    Signals,
};

/// Cover behavior behind the merge port.
#[derive(Debug)]
pub struct Cover;

impl CoverPort for Cover {
    fn partition_reports<'a>(
        &self,
        request: &'a MergeRequest,
        entries: &BTreeMap<&str, &MatrixEntry>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    ) -> Partition<'a> {
        super::cover::partition_reports(request, entries, signals, miss_reasons)
    }

    fn cover_entry(
        &self,
        request: &MergeRequest,
        entry: &MatrixEntry,
        report: &MatrixReport,
        obligations: &BTreeMap<&str, &str>,
        sinks: &mut CoverSinks<'_>,
    ) -> Result<bool, OrchestratorError> {
        super::cover::cover_entry(request, entry, report, obligations, sinks)
    }

    fn revalidate_coverage(
        &self,
        plan: &Plan,
        manifest: Option<&BaselineManifest>,
        signals: &mut Signals,
        miss_reasons: &mut BTreeSet<String>,
    ) {
        super::cover::revalidate_coverage(plan, manifest, signals, miss_reasons);
    }

    fn check_entry_shards(
        &self,
        bases: &BTreeSet<String>,
        empty_count: u32,
        proofs: &[ShardProof],
        obligations: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        super::cover::shard::check_entry_shards(bases, empty_count, proofs, obligations)
    }

    fn validate_budgets(&self, limits: &ResourceLimits) -> Result<(), String> {
        super::cover::shard::validate_budgets(limits)
    }
}
