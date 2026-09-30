//! Final-gate `needs` channel: validator inventory plus conclusions.
//!
//! The renderer emits the finalized `needs` set twice: the runtime
//! conclusions as JSON plus the static expected inventory. Assembly
//! declares the expected set (minus the matrix-driver job) as the
//! required validator inventory and fails closed when observed
//! conclusions diverge from it. No hardcoded job list lives here:
//! whatever the committed workflow needs, the merge requires.

use velnor_actions_workflow_renderer::render::TASK_JOB_ID;

/// Environment channel carrying the final gate's `needs` conclusions.
pub(crate) const NEEDS_ENV: &str = "VELNOR_NEEDS_JSON";
/// Environment channel carrying the rendered expected inventory.
pub(crate) const NEEDS_EXPECTED_ENV: &str = "VELNOR_NEEDS_EXPECTED";

/// Required inventory plus observed results from the needs channel.
///
/// Accepts direct conclusions and `toJSON(needs)` objects; the
/// matrix-driver job is excluded because per-leg reports prove its legs.
/// The inventory binds to the rendered expected set, never to whatever
/// the run observed: a dropped validator (missing from conclusions) or
/// an unexpected one fails closed with `needs_inventory_mismatch`
/// instead of silently shrinking the required set. A missing or
/// unparsable channel yields an empty inventory plus an explicit error,
/// failing the verdict closed, never silent.
pub(crate) fn parse_needs(
    needs: Option<&str>,
    expected: Option<&str>,
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
    let Some(want) = parse_expected(expected, errors) else {
        return (Vec::new(), Vec::new());
    };
    let mut observed = Vec::new();
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
        observed.push(job_id.clone());
        results.push(serde_json::json!({"job_id": job_id, "conclusion": conclusion}));
    }
    observed.sort();
    results.sort_by(|left, right| report_job(left).cmp(report_job(right)));
    if observed != want {
        errors.push("needs_inventory_mismatch".to_owned());
    }
    (want, results)
}

/// Rendered expected inventory minus the matrix-driver job.
///
/// The renderer lists every job except the gate; the driver job needs
/// no conclusion because per-leg reports prove its legs. Missing,
/// unparsable, or empty expectations fail closed: without them the
/// inventory would again derive from observation alone.
fn parse_expected(expected: Option<&str>, errors: &mut Vec<String>) -> Option<Vec<String>> {
    let Some(text) = expected.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_needs_expected".to_owned());
        return None;
    };
    let Ok(parsed) = serde_json::from_str::<Vec<String>>(text) else {
        errors.push("missing_needs_expected".to_owned());
        return None;
    };
    let mut want: Vec<String> = parsed
        .into_iter()
        .filter(|job_id| job_id != TASK_JOB_ID)
        .collect();
    want.sort();
    want.dedup();
    if want.is_empty() {
        errors.push("missing_needs_expected".to_owned());
        return None;
    }
    Some(want)
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
