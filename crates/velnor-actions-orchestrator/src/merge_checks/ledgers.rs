// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::workflow::cache_context::{UnitLedgerV1, UnitOutcome, UnitResultV1};
use velnor_actions_contract::{JobConclusion, JobDisposition, ObligationDecision, Plan, WorkflowEvent};

use super::super::MergeRequest;
use crate::cover::Signals;

pub(crate) fn check_group_ledgers(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if request.group_ledgers.is_empty() {
        return;
    }
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
        JobConclusion::Success => ledger
            .ordered_results
            .iter()
            .all(UnitResultV1::is_successful_completion),
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

fn check_group_ledger_coverage(
    plan: &Plan,
    ledgers: &[UnitLedgerV1],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let mut claimed = BTreeSet::new();
    for ledger in ledgers {
        for result in &ledger.ordered_results {
            if !plan.task_ids.contains(&result.unit_id) {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
            if !claimed.insert(result.unit_id.as_str()) {
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
            && !claimed.contains(obligation.task_id.as_str())
        {
            signals.planning_failed = true;
            miss_reasons.insert("no_entry".to_owned());
        }
    }
    for job in &plan.jobs {
        let count = ledgers
            .iter()
            .filter(|ledger| ledger.job.job_key == job.job_id)
            .count();
        match job.disposition {
            JobDisposition::Run => {
                if count == 0 {
                    signals.planning_failed = true;
                    miss_reasons.insert("no_entry".to_owned());
                }
            }
            JobDisposition::NotAffected => {
                if count > 0 {
                    signals.planning_failed = true;
                    miss_reasons.insert("gate_mismatch".to_owned());
                }
            }
        }
    }
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
