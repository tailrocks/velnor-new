use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::EvidenceError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExecutionKey {
    pub source: String,

    pub attempt: u64,

    pub plan: String,

    pub profile: String,

    pub logical_job: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedItem {
    pub key: ExecutionKey,

    pub artifact_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedExecutionSet {
    pub items: Vec<ExpectedItem>,
}

impl ExpectedExecutionSet {
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conclusion {
    Success,

    Skipped,

    Cancelled,

    TimedOut,

    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveSafety {
    Safe,

    Traversal,

    Symlink,

    CaseCollision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedExecutionReport {
    pub key: ExecutionKey,

    pub artifact_id: String,

    pub conclusion: Conclusion,

    pub runner_known: bool,

    pub cached_success: bool,

    pub archive: ArchiveSafety,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedJobCensus {
    pub complete: bool,

    pub omitted_page: bool,

    pub success_on_expected_runner: BTreeSet<ExecutionKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParityProof {
    pub lanes: usize,
}

pub fn verify_complete_results(
    expected: &ExpectedExecutionSet,
    observed: &[VerifiedExecutionReport],
    github: &VerifiedJobCensus,
) -> Result<ParityProof, EvidenceError> {
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
