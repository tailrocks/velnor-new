//! Per-key Mise-cache writer election (P08 race closure).
//!
//! Split from `cache_p08` (size gate): exactly one saver per built-in
//! cache key, elected after every setup step is inserted.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepKind};

use crate::setup::MISE_ACTION_NAME;

/// Elect one Mise-cache writer per cache key across jobs.
///
/// Every qualified setup saves on push by default, so jobs sharing one
/// key race concurrent saves of the same entry. Exactly one job per key
/// keeps the push-gated `cache_save`; the rest stand down to
/// restore-only (`cache_save: "false"`). The plan job wins a shared key
/// (it runs first and owns the trusted-writer role); a sole owner's key
/// keeps its writer; any other shared key goes to the lowest job ID, so
/// the election stays deterministic across renders.
pub fn elect_mise_cache_writers(jobs: &mut BTreeMap<String, Job>) {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, job) in jobs.iter() {
        if let Some(key) = setup_cache_key(job) {
            by_key.entry(key).or_default().push(id.clone());
        }
    }
    for owners in by_key.values() {
        let winner = owners
            .iter()
            .find(|id| id.as_str() == crate::render::PLAN_JOB_ID)
            .or_else(|| owners.iter().min())
            .map(String::as_str)
            .unwrap_or_default();
        for id in owners {
            if id != winner
                && let Some(job) = jobs.get_mut(id)
            {
                demote_setup_writer(job);
            }
        }
    }
}

/// This job's Mise built-in cache key, when its setup carries one.
fn setup_cache_key(job: &Job) -> Option<String> {
    job.steps.iter().find_map(|step| {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            return None;
        };
        if !uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            return None;
        }
        with.get("cache_key").cloned()
    })
}

/// Stand one setup down to restore-only (`cache_save: "false"`).
///
/// Only a push-gated saver demotes; foreign or already-false values
/// stay untouched so the election never rewrites unknown shapes.
fn demote_setup_writer(job: &mut Job) {
    for step in &mut job.steps {
        let StepKind::Action { uses, with, .. } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            continue;
        }
        if with.get("cache_save").is_some_and(|value| {
            value == velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION_EXPR
        }) {
            with.insert("cache_save".to_owned(), "false".to_owned());
        }
    }
}
