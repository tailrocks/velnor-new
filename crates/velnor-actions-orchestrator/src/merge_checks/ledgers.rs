// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_context::{
    ExecutionBranch, UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::workflow::crate_job::{GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX};
use velnor_actions_contract::{JobConclusion, JobDisposition, ObligationDecision, Plan, WorkflowEvent};

use super::super::MergeRequest;
use crate::cover::Signals;

pub(crate) fn check_group_ledgers(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let mut ledgers = Vec::new();
    for raw in &request.group_ledgers {
        if let Some(ledger) = parse_group_ledger(raw) {
            ledgers.push(ledger);
        } else {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
    }
    check_group_ledger_bindings(plan, request, &ledgers, signals, miss_reasons);
    check_group_ledger_coverage(plan, request, &ledgers, signals, miss_reasons);
}

fn ledger_group_key(job_key: &str) -> (&str, Option<ExecutionBranch>) {
    if let Some(group) = job_key.strip_suffix(GROUP_READER_SUFFIX) {
        (group, Some(ExecutionBranch::Reader))
    } else if let Some(group) = job_key.strip_suffix(GROUP_WRITER_SUFFIX) {
        (group, Some(ExecutionBranch::Writer))
    } else {
        (job_key, None)
    }
}

fn parse_group_ledger(raw: &serde_json::Value) -> Option<UnitLedgerV1> {
    let text = serde_json::to_string(raw).ok()?;
    UnitLedgerV1::parse_json(&text).ok()
}

fn check_group_ledger_bindings(
    plan: &Plan,
    request: &MergeRequest,
    ledgers: &[UnitLedgerV1],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let planned: BTreeSet<&str> = plan
        .jobs
        .iter()
        .map(|job| job.job_id.as_str())
        .collect();
    let mut seen_jobs = BTreeSet::new();
    let mut run_identity = None;
    for ledger in ledgers {
        if !seen_jobs.insert(ledger.job.job_key.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        if !request.required_job_ids.contains(&ledger.job.job_key) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        match run_identity {
            None => run_identity = Some((ledger.job.run_id, ledger.job.run_attempt)),
            Some(identity) => {
                if identity != (ledger.job.run_id, ledger.job.run_attempt) {
                    signals.planning_failed = true;
                    miss_reasons.insert("cache_corrupt".to_owned());
                }
            }
        }
        if ledger.job.source_sha != plan.head {
            signals.planning_failed = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
        }
        if ledger.job.event != group_event_name(plan.event) {
            signals.planning_failed = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
        }
        let (group, branch) = ledger_group_key(ledger.job.job_key.as_str());
        if !planned.contains(group) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        if branch.is_some_and(|branch| branch != ledger.execution_branch) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        check_group_ledger_conclusion(request, ledger, signals, miss_reasons);
    }
}

fn check_group_ledger_conclusion(
    request: &MergeRequest,
    ledger: &UnitLedgerV1,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let Some(record) = request
        .required_jobs
        .iter()
        .find(|job| job.job_id == ledger.job.job_key)
    else {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
        return;
    };
    let coherent = match record.conclusion {
        JobConclusion::Success => {
            if ledger
                .ordered_results
                .iter()
                .any(|result| result.outcome == UnitOutcome::Failed)
            {
                false
            } else if ledger_covers_group(ledger) {
                // Complete execution or complete cache reuse: a warm writer
                // that observed complete coverage prepares nothing and carries
                // the group with cache hits. That is coverage, not corruption.
                true
            } else if ledger_defers_preparation(ledger) {
                // Honest-but-incomplete evidence: the executing branch deferred
                // units to its sibling. Coverage routes this to `needs_prepare`;
                // it is not corruption.
                return;
            } else {
                false
            }
        }
        JobConclusion::Failure => ledger
            .ordered_results
            .iter()
            .any(|result| result.outcome == UnitOutcome::Failed),
        JobConclusion::Cancelled
        | JobConclusion::Skipped
        | JobConclusion::Neutral
        | JobConclusion::Missing => false,
    };
    if !coherent {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

fn ledger_complete(ledger: &UnitLedgerV1) -> bool {
    ledger
        .ordered_results
        .iter()
        .all(UnitResultV1::is_successful_completion)
}

fn ledger_covers_group(ledger: &UnitLedgerV1) -> bool {
    ledger
        .ordered_results
        .iter()
        .all(|result| result.is_successful_completion() || result.outcome == UnitOutcome::CacheHit)
}

fn ledger_defers_preparation(ledger: &UnitLedgerV1) -> bool {
    !ledger_complete(ledger)
        && ledger.ordered_results.iter().all(|result| {
            result.outcome == UnitOutcome::NeedsPrepare || result.is_successful_completion()
        })
}

#[derive(Default)]
struct GroupLedgerStats {
    reader_ledgers: usize,
    writer_ledgers: usize,
    reader_complete: usize,
    writer_complete: usize,
    defers_preparation: bool,
}

fn group_ledger_stats(ledgers: &[UnitLedgerV1]) -> BTreeMap<&str, GroupLedgerStats> {
    let mut per_group: BTreeMap<&str, GroupLedgerStats> = BTreeMap::new();
    for ledger in ledgers {
        let (group, _) = ledger_group_key(ledger.job.job_key.as_str());
        let stats = per_group.entry(group).or_default();
        match ledger.execution_branch {
            ExecutionBranch::Reader => {
                stats.reader_ledgers += 1;
                if ledger_covers_group(ledger) {
                    stats.reader_complete += 1;
                }
            }
            ExecutionBranch::Writer => {
                stats.writer_ledgers += 1;
                if ledger_covers_group(ledger) {
                    stats.writer_complete += 1;
                }
            }
        }
        if ledger_defers_preparation(ledger) {
            stats.defers_preparation = true;
        }
    }
    per_group
}

fn resolve_group_winner(stats: &GroupLedgerStats, writer_wins: bool) -> Option<ExecutionBranch> {
    if stats.writer_complete > 0 && (writer_wins || stats.reader_complete == 0) {
        Some(ExecutionBranch::Writer)
    } else if stats.reader_complete > 0 {
        Some(ExecutionBranch::Reader)
    } else {
        None
    }
}

fn check_group_ledger_coverage(
    plan: &Plan,
    request: &MergeRequest,
    ledgers: &[UnitLedgerV1],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let branch_groups: BTreeSet<&str> = plan
        .jobs
        .iter()
        .filter(|job| is_branch_group(request, job.job_id.as_str()))
        .map(|job| job.job_id.as_str())
        .collect();
    let per_group = group_ledger_stats(ledgers);
    // Only a push can execute both branches: the writer (later, trusted)
    // wins over the reader. Off-push the writer cannot run, so a complete
    // writer ledger next to a complete reader ledger is forged evidence and
    // stays `cache_corrupt` through the per-group check below.
    let writer_wins = plan.event == WorkflowEvent::Push;
    let mut claimed = BTreeSet::new();
    for ledger in ledgers {
        let (group, branch) = ledger_group_key(ledger.job.job_key.as_str());
        // A complete ledger supersedes its sibling branch: the losing branch
        // re-states the same units, so its claims are not duplicate evidence.
        let superseded = branch.is_some_and(|branch| {
            per_group.get(group).is_some_and(|stats| {
                resolve_group_winner(stats, writer_wins)
                    .is_some_and(|winner| winner != branch)
            })
        });
        for result in &ledger.ordered_results {
            if !plan.task_ids.contains(&result.unit_id) {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
            if !superseded && !claimed.insert(result.unit_id.as_str()) {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
            if plan.obligations.iter().any(|obligation| {
                obligation.task_id == result.unit_id
                    && obligation.decision != ObligationDecision::Execute
            }) {
                signals.planning_failed = true;
                miss_reasons.insert("gate_mismatch".to_owned());
            }
        }
    }
    for obligation in &plan.obligations {
        if obligation.decision == ObligationDecision::Execute
            && branch_groups.contains(obligation.job_id.as_str())
            && !claimed.contains(obligation.task_id.as_str())
        {
            signals.planning_failed = true;
            miss_reasons.insert("no_entry".to_owned());
        }
    }
    for job in &plan.jobs {
        if !is_branch_group(request, job.job_id.as_str()) {
            continue;
        }
        let stats = per_group.get(job.job_id.as_str());
        match job.disposition {
            JobDisposition::Run => {
                let Some(stats) = stats else {
                    signals.planning_failed = true;
                    miss_reasons.insert("no_entry".to_owned());
                    continue;
                };
                // Exactly one covering ledger carries the group: on push the
                // writer wins over the reader and the superseded sibling is
                // silent. Complete means executed or cache-hit: a warm writer
                // that reused every unit still carries the group. Off-push the
                // writer cannot execute, so two covering ledgers stay corrupt.
                // A group with evidence but no covering ledger is missing its
                // writer preparation, not corrupt.
                if stats.writer_complete > 0 && stats.reader_complete > 0 && !writer_wins {
                    signals.planning_failed = true;
                    miss_reasons.insert("cache_corrupt".to_owned());
                } else if resolve_group_winner(stats, writer_wins).is_none()
                    && stats.defers_preparation
                {
                    signals.needs_prepare = true;
                    miss_reasons.insert("needs_prepare".to_owned());
                }
            }
            JobDisposition::NotAffected => {
                if stats.is_some_and(|stats| stats.reader_ledgers + stats.writer_ledgers > 0) {
                    signals.planning_failed = true;
                    miss_reasons.insert("gate_mismatch".to_owned());
                }
            }
        }
    }
}

fn is_branch_group(request: &MergeRequest, group: &str) -> bool {
    let reader = format!("{group}{GROUP_READER_SUFFIX}");
    let writer = format!("{group}{GROUP_WRITER_SUFFIX}");
    request
        .required_job_ids
        .iter()
        .any(|id| id == &reader || id == &writer)
}

fn group_event_name(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::PullRequest => "pull_request",
        WorkflowEvent::Push => "push",
        WorkflowEvent::MergeGroup => "merge_group",
        WorkflowEvent::Local => "local",
        WorkflowEvent::Fork => "fork",
        WorkflowEvent::Qualification => "qualification",
    }
}
