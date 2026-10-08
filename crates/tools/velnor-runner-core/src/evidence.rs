//! Fail-closed paired-execution check. Duplicates are rejected before insert.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::EvidenceError;

/// Identity of one lane of one logical job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExecutionKey {
    /// Git source SHA the planner selected.
    pub source: String,
    /// Workflow run attempt.
    pub attempt: u64,
    /// Plan digest.
    pub plan: String,
    /// Hosted or scale-set profile.
    pub profile: String,
    /// Logical job id, not the rendered display name.
    pub logical_job: String,
}

/// One expected lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedItem {
    /// Lane identity.
    pub key: ExecutionKey,
    /// Immutable artifact id.
    pub artifact_id: String,
}

/// Ledger built before fan-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedExecutionSet {
    /// Expected lanes.
    pub items: Vec<ExpectedItem>,
}

impl ExpectedExecutionSet {
    /// Number of expected lanes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when the ledger is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn classify(&self, report: &VerifiedExecutionReport) -> Result<(), EvidenceError> {
        if self.items.iter().any(|item| item.key == report.key) {
            return Ok(());
        }
        if self.items.iter().any(|item| {
            item.key.source == report.key.source && item.key.attempt != report.key.attempt
        }) {
            return Err(EvidenceError::NotProven("wrong_attempt"));
        }
        if same_job(self, report, |item| item.key.source != report.key.source) {
            return Err(EvidenceError::NotProven("wrong_source"));
        }
        if same_job(self, report, |item| item.key.plan != report.key.plan) {
            return Err(EvidenceError::NotProven("wrong_plan"));
        }
        if same_job(self, report, |item| item.key.profile != report.key.profile) {
            return Err(EvidenceError::NotProven("wrong_profile"));
        }
        Err(EvidenceError::NotProven("missing_lane"))
    }
}

/// GitHub job conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conclusion {
    /// Job succeeded.
    Success,
    /// Job was skipped.
    Skipped,
    /// Job was cancelled.
    Cancelled,
    /// Job timed out.
    TimedOut,
    /// Job failed.
    Failed,
}

/// Archive inspection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveSafety {
    /// Regular files only.
    Safe,
    /// Path escaped the archive root.
    Traversal,
    /// A symlink or hardlink was present.
    Symlink,
    /// Two paths collide under case folding.
    CaseCollision,
}

/// One observed lane report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedExecutionReport {
    /// Lane identity.
    pub key: ExecutionKey,
    /// Artifact id the producer returned.
    pub artifact_id: String,
    /// Authoritative conclusion.
    pub conclusion: Conclusion,
    /// Runner mapped to the expected scale set or hosted label.
    pub runner_known: bool,
    /// True when a cached success was substituted for execution.
    pub cached_success: bool,
    /// Archive inspection.
    pub archive: ArchiveSafety,
}

/// Complete job census for one run attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedJobCensus {
    /// False when a page was not fetched.
    pub complete: bool,
    /// True when the client stopped before the last page.
    pub omitted_page: bool,
    /// Keys GitHub reports as success on the expected runner.
    pub success_on_expected_runner: BTreeSet<ExecutionKey>,
}

/// Proof that the two lanes agree. Constructed only by [`verify_complete_results`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParityProof {
    /// Number of lanes checked.
    pub lanes: usize,
}

/// Index reports, then require a complete census and real execution.
///
/// # Errors
///
/// Returns [`EvidenceError::DuplicateExecution`] before inserting a duplicate key,
/// or [`EvidenceError::NotProven`] for every other fail-closed case.
pub fn verify_complete_results(
    expected: &ExpectedExecutionSet,
    observed: &[VerifiedExecutionReport],
    github: &VerifiedJobCensus,
) -> Result<ParityProof, EvidenceError> {
    if expected.is_empty() {
        return Err(EvidenceError::NotProven("empty_expected_set"));
    }
    let indexed = index_reports(expected, observed)?;
    check_census(github)?;
    check_items(expected, &indexed, github)?;
    Ok(ParityProof {
        lanes: indexed.len(),
    })
}

fn index_reports<'a>(
    expected: &ExpectedExecutionSet,
    observed: &'a [VerifiedExecutionReport],
) -> Result<BTreeMap<ExecutionKey, &'a VerifiedExecutionReport>, EvidenceError> {
    let mut indexed = BTreeMap::new();
    for report in observed {
        expected.classify(report)?;
        match indexed.entry(report.key.clone()) {
            Entry::Vacant(slot) => {
                slot.insert(report);
            }
            Entry::Occupied(_) => return Err(EvidenceError::DuplicateExecution),
        }
    }
    if indexed.len() != expected.len() {
        return Err(EvidenceError::IncompleteExecutionSet);
    }
    Ok(indexed)
}

fn check_census(github: &VerifiedJobCensus) -> Result<(), EvidenceError> {
    if github.omitted_page {
        return Err(EvidenceError::NotProven("omitted_page"));
    }
    if !github.complete {
        return Err(EvidenceError::NotProven("incomplete_census"));
    }
    Ok(())
}

fn check_items(
    expected: &ExpectedExecutionSet,
    indexed: &BTreeMap<ExecutionKey, &VerifiedExecutionReport>,
    github: &VerifiedJobCensus,
) -> Result<(), EvidenceError> {
    for item in &expected.items {
        let report = indexed
            .get(&item.key)
            .ok_or(EvidenceError::MissingExecution)?;
        check_report(item, report, github)?;
    }
    Ok(())
}

fn check_report(
    item: &ExpectedItem,
    report: &VerifiedExecutionReport,
    github: &VerifiedJobCensus,
) -> Result<(), EvidenceError> {
    if report.archive != ArchiveSafety::Safe {
        return Err(EvidenceError::NotProven(archive_reason(report.archive)));
    }
    if report.cached_success {
        return Err(EvidenceError::NotProven("cached_success"));
    }
    if report.conclusion != Conclusion::Success {
        return Err(EvidenceError::NotProven("bad_conclusion"));
    }
    if report.artifact_id.is_empty() {
        return Err(EvidenceError::NotProven("missing_artifact"));
    }
    if report.artifact_id != item.artifact_id {
        return Err(EvidenceError::NotProven("swapped_artifact"));
    }
    if !report.runner_known || !github.success_on_expected_runner.contains(&item.key) {
        return Err(EvidenceError::NotProven("unknown_runner"));
    }
    Ok(())
}

fn same_job(
    expected: &ExpectedExecutionSet,
    report: &VerifiedExecutionReport,
    differ: impl Fn(&ExpectedItem) -> bool,
) -> bool {
    expected
        .items
        .iter()
        .any(|item| item.key.logical_job == report.key.logical_job && differ(item))
}

fn archive_reason(archive: ArchiveSafety) -> &'static str {
    match archive {
        ArchiveSafety::Safe => "archive",
        ArchiveSafety::Traversal => "traversal",
        ArchiveSafety::Symlink => "symlink",
        ArchiveSafety::CaseCollision => "case_collision",
    }
}
