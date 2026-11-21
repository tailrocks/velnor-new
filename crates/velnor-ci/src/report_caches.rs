// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::report::{Cache, Hit, SaveOutcome};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachePlan {
    family: String,
    root: Option<String>,
    restore_step: String,
    save_step: String,
    #[serde(default)]
    not_generated: Option<String>,
}

pub(crate) fn privacy_unavailable(steps: &Value) -> bool {
    let step = &steps["private_repo"];
    if step.is_null() || step["outcome"] == "skipped" {
        return false;
    }
    let outputs = &step["outputs"];
    step["outcome"] != "success"
        || outputs["unavailable"] == "true"
        || !matches!(outputs["private"].as_str(), Some("true" | "false"))
}

pub(crate) fn observe_caches(
    plans: Vec<CachePlan>,
    steps: &Value,
    saving: bool,
) -> (Vec<Cache>, Vec<String>) {
    let mut errors = Vec::new();
    if !steps.is_object() {
        errors.push("Cache restore evidence must be a steps object".to_owned());
    }
    let caches = plans
        .into_iter()
        .map(|plan| {
            let evidence = restore(&steps[&plan.restore_step]);
            if let Err(error) = &evidence {
                errors.push(format!("{}: {error}", plan.restore_step));
            }
            let (requested_key, matched_key, hit) = evidence.unwrap_or_default();
            let save = if saving {
                match steps[&plan.save_step]["outcome"].as_str() {
                    Some("failure" | "cancelled") => SaveOutcome::Failed,
                    Some("success") => SaveOutcome::Unknown,
                    _ => SaveOutcome::NotAttempted,
                }
            } else {
                SaveOutcome::NotAttempted
            };
            Cache {
                family: plan.family,
                root: plan.root,
                restore_step: plan.restore_step,
                save_step: plan.save_step,
                requested_key,
                matched_key,
                hit: hit.unwrap_or(Hit::Miss),
                bytes: None,
                usable: false,
                usable_same_lock: false,
                save,
                not_generated: plan.not_generated,
                entry_before_job: None,
            }
        })
        .collect();
    (caches, errors)
}

fn restore(step: &Value) -> Result<(String, Option<String>, Option<Hit>), &'static str> {
    let outputs = step["outputs"]
        .as_object()
        .ok_or("Cache restore outputs are unavailable")?;
    if step["outcome"] == "skipped" && outputs.is_empty() {
        return Ok((String::new(), None, Some(Hit::Miss)));
    }
    if step["outcome"] != "success" {
        return Err("Cache restore outcome is unavailable or unsuccessful");
    }
    let requested = outputs
        .get("cache-primary-key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .ok_or("Cache restore primary key is unavailable")?;
    for key in ["cache-matched-key", "cache-hit"] {
        if outputs.get(key).is_some_and(|value| !value.is_string()) {
            return Err("Cache restore output must be a string");
        }
    }
    let matched = outputs
        .get("cache-matched-key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty());
    let exact = outputs
        .get("cache-hit")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !["", "true", "false"].contains(&exact)
        || exact == "true" && matched.is_none_or(|key| !key.eq_ignore_ascii_case(requested))
    {
        return Err("Cache restore hit evidence is inconsistent");
    }
    let hit = match matched {
        Some(key) if exact == "true" || key == requested => Hit::Exact,
        Some(_) => Hit::Fallback,
        None => Hit::Miss,
    };
    Ok((requested.to_owned(), matched.map(str::to_owned), Some(hit)))
}

#[cfg(test)]
mod tests {
    use super::{CachePlan, observe_caches, privacy_unavailable};
    use crate::report::{LaneMetrics, SaveOutcome, check};
    use serde_json::json;

    #[test]
    fn a_restore_and_save_skipped_by_the_privacy_verdict_fail_no_check() {
        let plans = vec![CachePlan {
            family: "sources".to_owned(),
            root: None,
            restore_step: "restore".to_owned(),
            save_step: "save".to_owned(),
            not_generated: None,
        }];
        let steps = json!({
            "restore": {"outcome": "skipped", "outputs": {}},
            "save": {"outcome": "skipped", "outputs": {}},
        });
        let (caches, errors) = observe_caches(plans, &steps, true);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(caches[0].save, SaveOutcome::NotAttempted);
        let lane = json!({
            "schema": 1,
            "lane": "rust",
            "caches": caches,
            "invocations": [],
            "downloaded": 0,
            "first_cargo_seconds": null,
            "slowest_tests": [],
            "tests_run": null,
            "diagnostics": [],
            "violations": [],
        });
        let metrics: LaneMetrics =
            serde_json::from_value(lane).unwrap_or_else(|error| unreachable!("{error}"));
        let verdict = check(&[metrics]);
        assert!(
            verdict.is_ok(),
            "{}",
            verdict.err().map_or_else(String::new, |e| e.to_string())
        );
    }

    fn skipped_lane(privacy: serde_json::Value) -> LaneMetrics {
        let mut lane = json!({
            "schema": 1,
            "lane": "rust",
            "caches": [],
            "invocations": [],
            "downloaded": 0,
            "first_cargo_seconds": null,
            "slowest_tests": [],
            "tests_run": null,
            "diagnostics": [],
            "violations": [],
        });
        lane["privacy_unavailable"] = privacy;
        serde_json::from_value(lane).unwrap_or_else(|error| unreachable!("{error}"))
    }

    #[test]
    fn only_an_unavailable_privacy_lookup_is_carried_into_the_metrics() {
        let step = |outputs| json!({"private_repo": {"outcome": "success", "outputs": outputs}});
        assert!(privacy_unavailable(&step(
            json!({"private": "false", "unavailable": "true"})
        )));
        assert!(!privacy_unavailable(&step(json!({"private": "false"}))));
        assert!(!privacy_unavailable(&step(json!({"private": "true"}))));
        assert!(!privacy_unavailable(&json!({})));
    }

    #[test]
    fn a_private_repo_step_that_did_not_confirm_the_visibility_is_unavailable() {
        for (label, step) in [
            ("failed, no outputs", json!({"outcome": "failure"})),
            (
                "failed, empty outputs",
                json!({"outcome": "failure", "outputs": {}}),
            ),
            ("cancelled", json!({"outcome": "cancelled"})),
            (
                "succeeded without a verdict",
                json!({"outcome": "success", "outputs": {}}),
            ),
            (
                "succeeded with an unreadable verdict",
                json!({"outcome": "success", "outputs": {"private": ""}}),
            ),
            (
                "succeeded without an outcome",
                json!({"outputs": {"private": "false"}}),
            ),
        ] {
            assert!(
                privacy_unavailable(&json!({"private_repo": step})),
                "{label}"
            );
        }
    }

    #[test]
    fn a_privacy_step_skipped_by_the_lane_selection_is_not_unavailable() {
        let step = json!({"private_repo": {"outcome": "skipped", "outputs": {}}});
        assert!(!privacy_unavailable(&step));
    }

    #[test]
    fn an_unavailable_privacy_verdict_fails_the_cache_check() {
        let verdict = check(&[skipped_lane(json!(true))]);
        let message = verdict.err().map_or_default(|e| e.to_string());
        assert!(
            message.contains("privacy lookup was unavailable"),
            "{message}"
        );
        assert!(check(&[skipped_lane(json!(false))]).is_ok());
    }
}
