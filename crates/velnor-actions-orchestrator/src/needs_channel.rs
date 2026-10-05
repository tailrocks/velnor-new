//! Final-gate `needs` channel: validator inventory plus conclusions.
//!
//! The renderer emits the finalized `needs` set twice: runtime
//! conclusions as JSON plus a static digest of the expected IDs.
//! Assembly declares the observed set (minus the matrix-driver job) as
//! required validators and fails closed when its digest differs from
//! the committed workflow. No hardcoded job list lives here.
//!
//! D7 process control: the expected inventory is generated YAML, and
//! the freshness gate that would catch hand edits lives in the same
//! file it verifies — so a workflow edit that shrinks the inventory
//! AND neuters the freshness check is self-consistent. The control is
//! outside the code: generated workflow files change only through
//! reviewed PRs (repo `CODEOWNERS` plus branch protection), and
//! reviewers treat inventory/condition edits as security-sensitive.
//! The merge cannot distinguish legit regeneration from tampering;
//! review is the trust root for the committed `needs` set.

use velnor_actions_contract::{JobConclusion, is_valid_digest, needs_inventory_digest};
use velnor_actions_workflow_renderer::render::TASK_JOB_ID;

/// Environment channel carrying the final gate's `needs` conclusions.
pub(crate) const NEEDS_ENV: &str = "VELNOR_NEEDS_JSON";
// The expected-inventory channel (`VELNOR_NEEDS_EXPECTED`) is the
// contract single source [`velnor_actions_contract::NEEDS_EXPECTED_ENV`],
// imported by consumers directly; no local copy lives here.

/// Required inventory plus observed results from the needs channel.
///
/// Accepts direct conclusions and `toJSON(needs)` objects. It hashes
/// every observed key, including the matrix-driver job, against the
/// renderer's static digest before excluding the driver from required
/// validator results. A dropped or unexpected job therefore fails
/// closed without repeating the full inventory in generated YAML.
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
    let Some(want_digest) = parse_expected(expected, errors) else {
        return (Vec::new(), Vec::new());
    };
    let mut observed_keys = Vec::with_capacity(map.len());
    let mut observed = Vec::new();
    let mut results = Vec::new();
    for (job_id, entry) in map {
        if job_id.trim().is_empty() {
            errors.push("bad_needs_job".to_owned());
            continue;
        }
        observed_keys.push(job_id.clone());
        if job_id == TASK_JOB_ID {
            continue;
        }
        observed.push(job_id.clone());
        let Some(conclusion) = needs_conclusion(entry) else {
            errors.push(format!("bad_needs_result:{job_id}"));
            continue;
        };
        results.push(serde_json::json!({"job_id": job_id, "conclusion": conclusion.as_str()}));
    }
    observed_keys.sort();
    observed.sort();
    results.sort_by(|left, right| report_job(left).cmp(report_job(right)));
    if needs_inventory_digest(&observed_keys) != want_digest {
        errors.push("needs_inventory_mismatch".to_owned());
    }
    (observed, results)
}

/// Valid rendered digest of the complete `needs` key set.
///
/// Missing or malformed expectations fail closed: without them the
/// workflow inventory would again derive from observation alone.
fn parse_expected(expected: Option<&str>, errors: &mut Vec<String>) -> Option<String> {
    let Some(text) = expected.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_needs_expected".to_owned());
        return None;
    };
    if !is_valid_digest(text) {
        errors.push("missing_needs_expected".to_owned());
        return None;
    }
    Some(text.to_owned())
}

/// One needs conclusion: direct string or `{result}` object shape.
///
/// Only the closed GitHub `needs` result vocabulary passes; anything
/// else is a corrupt channel, never folded as success or failure.
fn needs_conclusion(entry: &serde_json::Value) -> Option<JobConclusion> {
    let raw = entry
        .as_str()
        .or_else(|| entry.get("result").and_then(serde_json::Value::as_str))?;
    match raw {
        "success" => Some(JobConclusion::Success),
        "failure" => Some(JobConclusion::Failure),
        "cancelled" => Some(JobConclusion::Cancelled),
        "skipped" => Some(JobConclusion::Skipped),
        _ => None,
    }
}

/// Sort key for one assembled job result; empty when the ID is absent.
fn report_job(result: &serde_json::Value) -> &str {
    result
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse results plus errors for one channel value.
    fn parsed(
        needs: Option<&str>,
        expected: Option<&str>,
    ) -> (Vec<String>, Vec<serde_json::Value>, Vec<String>) {
        let mut errors = Vec::new();
        let (inventory, results) = parse_needs(needs, expected, &mut errors);
        (inventory, results, errors)
    }

    #[test]
    fn direct_and_to_json_shapes_parse_and_sort() {
        let expected =
            needs_inventory_digest(&["alpha".to_owned(), "plan".to_owned(), "zeta".to_owned()]);
        let (inventory, results, errors) = parsed(
            Some(r#"{"zeta":"success","plan":{"result":"failure"},"alpha":{"result":"bogus"}}"#),
            Some(&expected),
        );
        assert_eq!(inventory, ["alpha", "plan", "zeta"]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["job_id"], "plan");
        assert_eq!(results[0]["conclusion"], "failure");
        assert_eq!(
            errors,
            ["bad_needs_result:alpha"],
            "unknown conclusions corrupt the channel"
        );
        let expected = needs_inventory_digest(&["a".to_owned(), "b".to_owned()]);
        let (inventory, _, errors) =
            parsed(Some(r#"{"a":"cancelled","b":"skipped"}"#), Some(&expected));
        assert_eq!(inventory, ["a", "b"]);
        assert!(errors.is_empty());
    }

    #[test]
    fn matrix_driver_job_is_excluded() {
        let channel = format!(r#"{{"plan":"success","{TASK_JOB_ID}":"success"}}"#);
        let expected = needs_inventory_digest(&["plan".to_owned(), TASK_JOB_ID.to_owned()]);
        let (inventory, results, errors) = parsed(Some(&channel), Some(&expected));
        assert_eq!(inventory, ["plan"]);
        assert_eq!(results.len(), 1);
        assert!(errors.is_empty());
    }

    #[test]
    fn matrix_driver_key_is_bound_before_its_result_is_excluded() {
        let with_driver = needs_inventory_digest(&["plan".to_owned(), TASK_JOB_ID.to_owned()]);
        let without_driver = needs_inventory_digest(&["plan".to_owned()]);
        let with_driver_channel = format!(r#"{{"plan":"success","{TASK_JOB_ID}":"success"}}"#);
        for (channel, expected) in [
            (r#"{"plan":"success"}"#, with_driver.as_str()),
            (with_driver_channel.as_str(), without_driver.as_str()),
        ] {
            let (_, results, errors) = parsed(Some(channel), Some(expected));
            assert_eq!(results.len(), 1, "the driver remains excluded from reports");
            assert!(
                errors.contains(&"needs_inventory_mismatch".to_owned()),
                "driver key presence must match the committed full-key digest: {channel}"
            );
        }
    }

    #[test]
    fn garbage_empty_and_bad_job_inputs_fail_closed() {
        for needs in [None, Some(""), Some("   ")] {
            let (inventory, results, errors) = parsed(needs, None);
            assert!(inventory.is_empty());
            assert!(results.is_empty());
            assert_eq!(errors, ["missing_needs_channel"]);
        }
        for needs in ["not json", "[1,2]", "42", r#""str""#] {
            let (inventory, results, errors) = parsed(Some(needs), None);
            assert!(inventory.is_empty());
            assert!(results.is_empty());
            assert_eq!(errors, ["unparsable_needs"], "{needs}");
        }
        let (inventory, results, errors) = parsed(Some("{}"), None);
        assert!(inventory.is_empty());
        assert!(results.is_empty());
        assert_eq!(errors, ["empty_needs"]);
        let expected = needs_inventory_digest(&["ok".to_owned()]);
        let (inventory, _, errors) =
            parsed(Some(r#"{"":"success","ok":"success"}"#), Some(&expected));
        assert_eq!(inventory, ["ok"]);
        assert_eq!(errors, ["bad_needs_job"]);
        let expected = needs_inventory_digest(&["a".to_owned(), "b".to_owned()]);
        let (_, _, errors) = parsed(Some(r#"{"a":{},"b":{"result":7}}"#), Some(&expected));
        assert_eq!(errors, ["bad_needs_result:a", "bad_needs_result:b"]);
    }

    #[test]
    fn expected_digest_binds_missing_extra_and_malformed_needs_keys() {
        let expected = needs_inventory_digest(&["plan".to_owned(), "lint".to_owned()]);
        for channel in [
            r#"{"plan":"success"}"#,
            r#"{"plan":"success","lint":"success","extra":"success"}"#,
        ] {
            let (_, _, errors) = parsed(Some(channel), Some(&expected));
            assert!(errors.contains(&"needs_inventory_mismatch".to_owned()));
        }
        let (_, _, errors) = parsed(Some(r#"{"plan":"success"}"#), Some("[\"plan\"]"));
        assert_eq!(errors, ["missing_needs_expected"]);
    }
}
