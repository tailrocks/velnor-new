use std::collections::BTreeSet;

use velnor_actions_contract::{
    FinalReport, JobConclusion, ObligationDecision, Plan, RequiredJobResult,
};

use super::MergeRequest;
use crate::cover::Signals;
use crate::internal::internal_contract;

pub(crate) fn check_required_evidence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    check_job_inventory(request, signals, miss_reasons);
    check_obligation_proofs(plan, signals, miss_reasons);
    if !request.assembly_errors.is_empty() {
        signals.planning_failed = true;
        miss_reasons.extend(assembly_tokens(&request.assembly_errors));
    }
}

fn check_job_inventory(
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if request.required_job_ids.is_empty() {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
        return;
    }
    let expected: BTreeSet<&str> = request
        .required_job_ids
        .iter()
        .map(String::as_str)
        .collect();
    if expected.len() != request.required_job_ids.len() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    let mut seen = BTreeSet::new();
    for job in &request.required_jobs {
        if !seen.insert(job.job_id.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        if !expected.contains(job.job_id.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
    }
    if expected.iter().any(|id| !seen.contains(id)) {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}

fn check_obligation_proofs(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::ReusedFromTaskCache => {
                signals.planning_failed = true;
                miss_reasons.insert("no_entry".to_owned());
            }
            ObligationDecision::Execute | ObligationDecision::NotAffected => {}
        }
    }
}

pub(crate) fn reported_job_results(request: &MergeRequest) -> Vec<RequiredJobResult> {
    let mut reported = request.required_jobs.clone();
    for id in &request.required_job_ids {
        if !reported.iter().any(|job| &job.job_id == id) {
            reported.push(RequiredJobResult {
                job_id: id.clone(),
                conclusion: JobConclusion::Missing,
            });
        }
    }
    reported.sort_by(|left, right| left.job_id.cmp(&right.job_id));
    reported
}

pub(crate) fn diagnostic_without_plan(
    request: &MergeRequest,
    tokens: BTreeSet<String>,
) -> Result<FinalReport, crate::OrchestratorError> {
    let mut report = FinalReport::without_plan(&request.run_key, reported_job_results(request))
        .map_err(internal_contract)?;
    let mut miss = assembly_tokens(&request.assembly_errors);
    miss.extend(tokens);
    report.miss_reasons = miss.into_iter().collect();
    report.validate().map_err(internal_contract)?;
    Ok(report)
}

fn assembly_tokens(errors: &[String]) -> BTreeSet<String> {
    errors
        .iter()
        .map(|error| {
            if error == "empty_needs" || error.starts_with("missing_needs") {
                "no_entry"
            } else if error.starts_with("missing_") {
                "source_missing"
            } else {
                "cache_corrupt"
            }
            .to_owned()
        })
        .collect()
}
