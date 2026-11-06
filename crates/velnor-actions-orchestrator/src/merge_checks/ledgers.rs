// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_context::{UnitLedgerV1, UnitOutcome, UnitResultV1};
use velnor_actions_contract::{
    JobConclusion, JobDisposition, ObligationDecision, Plan, WorkflowEvent, is_crate_job_id,
};

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
    check_group_ledger_coverage(plan, &ledgers, signals, miss_reasons);
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
    let planned: BTreeSet<&str> = plan.jobs.iter().map(|job| job.job_id.as_str()).collect();
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
        let group = ledger.job.job_key.as_str();
        if !planned.contains(group) || !is_crate_job_id(group) {
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
                true
            } else if ledger_defers_preparation(ledger) {
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
    covers: bool,
    defers_preparation: bool,
}

fn group_ledger_stats(ledgers: &[UnitLedgerV1]) -> BTreeMap<&str, GroupLedgerStats> {
    let mut per_group: BTreeMap<&str, GroupLedgerStats> = BTreeMap::new();
    for ledger in ledgers {
        let stats = per_group.entry(ledger.job.job_key.as_str()).or_default();
        stats.covers |= ledger_covers_group(ledger);
        stats.defers_preparation |= ledger_defers_preparation(ledger);
    }
    per_group
}

fn check_group_ledger_coverage(
    plan: &Plan,
    ledgers: &[UnitLedgerV1],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let groups: BTreeSet<&str> = plan
        .jobs
        .iter()
        .map(|job| job.job_id.as_str())
        .filter(|id| is_crate_job_id(id))
        .collect();
    let claimed = check_unit_claims(plan, ledgers, signals, miss_reasons);
    for obligation in &plan.obligations {
        if obligation.decision == ObligationDecision::Execute
            && groups.contains(obligation.job_id.as_str())
            && !claimed.contains(obligation.task_id.as_str())
        {
            signals.planning_failed = true;
            miss_reasons.insert("no_entry".to_owned());
        }
    }
    let per_group = group_ledger_stats(ledgers);
    for job in plan
        .jobs
        .iter()
        .filter(|job| groups.contains(job.job_id.as_str()))
    {
        let stats = per_group.get(job.job_id.as_str());
        match (job.disposition, stats) {
            (JobDisposition::Run, None) => {
                signals.planning_failed = true;
                miss_reasons.insert("no_entry".to_owned());
            }
            (JobDisposition::Run, Some(stats)) => {
                if !stats.covers && stats.defers_preparation {
                    signals.needs_prepare = true;
                    miss_reasons.insert("needs_prepare".to_owned());
                }
            }
            (JobDisposition::NotAffected, Some(_)) => {
                signals.planning_failed = true;
                miss_reasons.insert("gate_mismatch".to_owned());
            }
            (JobDisposition::NotAffected, None) => {}
        }
    }
}

fn check_unit_claims<'a>(
    plan: &Plan,
    ledgers: &'a [UnitLedgerV1],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> BTreeSet<&'a str> {
    let mut claimed = BTreeSet::new();
    for ledger in ledgers {
        for result in &ledger.ordered_results {
            claimed.insert(result.unit_id.as_str());
            let obligation = plan
                .obligations
                .iter()
                .find(|obligation| obligation.task_id == result.unit_id);
            if obligation.is_none_or(|obligation| obligation.job_id != ledger.job.job_key) {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
            if obligation
                .is_some_and(|obligation| obligation.decision != ObligationDecision::Execute)
            {
                signals.planning_failed = true;
                miss_reasons.insert("gate_mismatch".to_owned());
            }
        }
    }
    claimed
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
