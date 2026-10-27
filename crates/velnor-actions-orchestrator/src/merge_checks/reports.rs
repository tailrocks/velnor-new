// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ExecuteTaskRef, MatrixEntry, Plan, TaskReport, task_report_id_for_task,
};

use super::super::MergeRequest;
use crate::cover::Signals;
use crate::cover::shard::{check_entry_shards, validate_budgets};

pub(crate) fn plan_entries(plan: &Plan) -> BTreeMap<&str, &MatrixEntry> {
    plan.matrix
        .include
        .iter()
        .map(|entry| (entry.report_id.as_str(), entry))
        .collect()
}

fn reference_matches(reference: &[String], planned: &[String]) -> bool {
    let mut left = reference.to_vec();
    let mut right = planned.to_vec();
    left.sort();
    right.sort();
    left == right
}

pub(crate) fn check_plan_evidence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if request
        .limits
        .as_ref()
        .is_some_and(|limits| validate_budgets(limits).is_err())
    {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    if request
        .reference_task_ids
        .as_ref()
        .is_some_and(|reference| !reference_matches(reference, &plan.task_ids))
    {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

pub(crate) fn shards_failed(
    entry: &MatrixEntry,
    empty: u32,
    plan: &Plan,
    request: &MergeRequest,
) -> bool {
    let bases = sharded_bases(entry);
    if bases.is_empty() {
        return false;
    }
    let mut inputs = BTreeMap::new();
    for ob in &plan.obligations {
        inputs.insert(ob.task_id.clone(), ob.input_digest.clone());
    }
    check_entry_shards(&bases, empty, &request.shard_proofs, &inputs).is_err()
}

fn sharded_bases(entry: &MatrixEntry) -> BTreeSet<String> {
    let mut bases = BTreeSet::new();
    for task_ref in entry.execute_task_ids.tasks.values() {
        let ids = match task_ref {
            velnor_actions_contract::ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
            velnor_actions_contract::ExecuteTaskRef::Shards(ids) => ids.as_slice(),
        };
        for id in ids {
            if let Some((base, _, _)) = velnor_actions_contract::split_shard_suffix(id) {
                bases.insert(base.to_owned());
            }
        }
    }
    bases
}

pub(crate) fn plan_digests(plan: &Plan) -> BTreeMap<&str, &str> {
    plan.obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
}

pub(crate) fn expected_task_reports(
    plan: &Plan,
    run_key: &str,
) -> BTreeMap<String, (String, String)> {
    let digests = plan_digests(plan);
    let mut expected = BTreeMap::new();
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            for id in ids {
                let derived = digests.get(id.as_str()).and_then(|digest| {
                    task_report_id_for_task(run_key, &entry.matrix_key, digest).ok()
                });
                if let Some(report_id) = derived {
                    expected.insert(report_id, (id.clone(), entry.matrix_key.clone()));
                }
            }
        }
    }
    expected
}

pub(crate) struct TaskPartition<'a> {
    pub(crate) valid: BTreeMap<&'a str, &'a TaskReport>,
    pub(crate) malformed: u32,
    pub(crate) duplicates: u32,
}

pub(crate) fn partition_task_reports<'a>(
    plan: &Plan,
    request: &'a MergeRequest,
    expected: &BTreeMap<String, (String, String)>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> TaskPartition<'a> {
    let mut valid = BTreeMap::new();
    let mut malformed = 0u32;
    let mut duplicates = 0u32;
    for report in &request.task_reports {
        if report.run_key != request.run_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
            continue;
        }
        if report.validate().is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        let Some((expected_task_id, expected_matrix_key)) = expected.get(&report.task_report_id)
        else {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        if report.task_id != *expected_task_id || report.matrix_key != *expected_matrix_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        let Some(entry) = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.matrix_key == *expected_matrix_key)
        else {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        if report.validate_for_plan_entry(entry).is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if valid.contains_key(report.task_report_id.as_str()) {
            duplicates += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        valid.insert(report.task_report_id.as_str(), report);
    }
    TaskPartition {
        valid,
        malformed,
        duplicates,
    }
}
