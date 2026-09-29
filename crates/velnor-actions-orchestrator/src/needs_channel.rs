//! Final-gate `needs` channel: validator inventory plus conclusions.
//!
//! The renderer emits the finalized `needs` set as JSON; assembly
//! declares it as the required validator inventory. No hardcoded job
//! list lives here: whatever the workflow needs, the merge requires.

use velnor_actions_workflow_renderer::render::TASK_JOB_ID;

/// Environment channel carrying the final gate's `needs` conclusions.
pub(crate) const NEEDS_ENV: &str = "VELNOR_NEEDS_JSON";

/// Required inventory plus observed results from the needs channel.
///
/// Accepts direct conclusions and `toJSON(needs)` objects; the
/// matrix-driver job is excluded because per-leg reports prove its legs.
/// A missing or unparsable channel yields an empty inventory plus an
/// explicit error, failing the verdict closed, never silent.
pub(crate) fn parse_needs(
    needs: Option<&str>,
    errors: &mut Vec<String>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let Some(text) = needs.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_needs_channel".to_owned());
        return (Vec::new(), Vec::new());
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) else {
        errors.push("unparsable_needs".to_owned());
        return (Vec::new(), Vec::new());
    };
    let Some(map) = parsed.as_object() else {
        errors.push("unparsable_needs".to_owned());
        return (Vec::new(), Vec::new());
    };
    if map.is_empty() {
        errors.push("empty_needs".to_owned());
        return (Vec::new(), Vec::new());
    }
    let mut inventory = Vec::new();
    let mut results = Vec::new();
    for (job_id, entry) in map {
        if job_id.trim().is_empty() {
            errors.push("bad_needs_job".to_owned());
            continue;
        }
        if job_id == TASK_JOB_ID {
            continue;
        }
        let Some(conclusion) = needs_conclusion(entry) else {
            errors.push(format!("bad_needs_result:{job_id}"));
            continue;
        };
        inventory.push(job_id.clone());
        results.push(serde_json::json!({"job_id": job_id, "conclusion": conclusion}));
    }
    inventory.sort();
    results.sort_by(|left, right| report_job(left).cmp(report_job(right)));
    (inventory, results)
}

/// One needs conclusion: direct string or `{result}` object shape.
///
/// Only the closed GitHub `needs` result vocabulary passes; anything
/// else is a corrupt channel, never folded as success or failure.
fn needs_conclusion(entry: &serde_json::Value) -> Option<&str> {
    let raw = entry
        .as_str()
        .or_else(|| entry.get("result").and_then(serde_json::Value::as_str))?;
    matches!(raw, "success" | "failure" | "cancelled" | "skipped").then_some(raw)
}

/// Sort key for one assembled job result; empty when the ID is absent.
fn report_job(result: &serde_json::Value) -> &str {
    result
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}
