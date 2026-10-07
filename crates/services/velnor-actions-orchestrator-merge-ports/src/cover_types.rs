//! Cover-owned vocabulary shared with merge: signals, sinks, partition.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{MatrixReport, TaskReport};

/// Aggregated merge signals feeding result precedence.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five precedence signals read clearest as named bools"
)]
pub struct Signals {
    /// Structural validation failed.
    pub planning_failed: bool,
    /// A required task, job, or candidate failed.
    pub failed: bool,
    /// A required task, job, or candidate was cancelled.
    pub cancelled: bool,
    /// A required task was blocked (`not_selected`).
    pub blocked: bool,
    /// A report is missing, malformed, duplicated, skipped, or not run.
    pub not_run: bool,
}

/// Check-2 partition of submitted reports.
#[derive(Debug)]
pub struct Partition<'a> {
    /// First valid report per expected report ID.
    pub valid: BTreeMap<&'a str, &'a MatrixReport>,
    /// Reports failing validation or bound to another run.
    pub malformed: u32,
    /// Extra reports beyond the first per report ID.
    pub duplicates: u32,
}

/// Folded task counts from covered reports.
#[derive(Debug, Default)]
pub struct Fold {
    /// Reused tasks.
    pub reused: u32,
    /// Executed tasks.
    pub executed: u32,
    /// Empty-partition tasks.
    pub empty_partition: u32,
    /// Failed tasks.
    pub failed: u32,
    /// Cancelled tasks.
    pub cancelled: u32,
    /// Not-selected (blocked) tasks.
    pub blocked: u32,
}

/// Mutable merge sinks threaded through per-entry coverage checks.
#[derive(Debug)]
pub struct CoverSinks<'a> {
    /// Seen task-report IDs.
    pub seen_task_reports: &'a mut BTreeSet<String>,
    /// Partitioned per-task files keyed by task-report ID.
    pub task_files: &'a BTreeMap<&'a str, &'a TaskReport>,
    /// Folded task counts.
    pub fold: &'a mut Fold,
    /// Merge signals.
    pub signals: &'a mut Signals,
    /// Miss reasons for uncovered tasks.
    pub miss_reasons: &'a mut BTreeSet<String>,
}
