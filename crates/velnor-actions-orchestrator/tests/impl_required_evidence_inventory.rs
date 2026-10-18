//! Exact required-job inventory fixtures for merge evidence tests.

use serde_json::json;
use velnor_actions_contract::FinalStatus;
use velnor_actions_orchestrator::merge_internal;

/// Merge status for one hand-built request value.
pub(super) fn status_of(
    request: &serde_json::Value,
) -> Result<FinalStatus, Box<dyn std::error::Error>> {
    Ok(
        serde_json::from_str::<velnor_actions_contract::FinalReport>(&merge_internal(
            &request.to_string(),
        )?)?
        .status,
    )
}

/// Required inventory includes workflow validators and every matrix job.
pub(super) fn required_job_ids(
    plan: &velnor_actions_contract::Plan,
    extra: &[&str],
) -> Vec<String> {
    let mut ids = vec!["plan".to_owned()];
    ids.extend(plan.matrix.include.iter().map(|entry| entry.job_id.clone()));
    ids.extend(extra.iter().map(|id| (*id).to_owned()));
    ids.sort();
    ids.dedup();
    ids
}

/// Successful results for all inventory IDs except explicitly omitted IDs.
pub(super) fn successful_results_except(
    ids: &[String],
    omitted: &[&str],
) -> Vec<serde_json::Value> {
    ids.iter()
        .filter(|id| !omitted.contains(&id.as_str()))
        .map(|job_id| json!({"job_id": job_id, "conclusion": "success"}))
        .collect()
}

/// Job results with one selected conclusion and successes for the rest.
pub(super) fn results_with_conclusion(
    ids: &[String],
    selected: &str,
    conclusion: &str,
) -> Vec<serde_json::Value> {
    ids.iter()
        .map(|job_id| {
            let result = if job_id == selected {
                conclusion
            } else {
                "success"
            };
            json!({"job_id": job_id, "conclusion": result})
        })
        .collect()
}
