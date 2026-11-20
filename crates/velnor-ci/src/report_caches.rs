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
    use super::{CachePlan, observe_caches};
    use crate::report::{LaneMetrics, SaveOutcome, check};
    use serde_json::json;

    /// A base cache gated on the privacy verdict is skipped in a public repository: GitHub
    /// records the restore and the save as `skipped` with no outputs. That is "not attempted",
    /// so the cache telemetry must not fail the check.
    #[test]
    fn a_restore_and_save_skipped_by_the_privacy_verdict_fail_no_check() {
        let plans = vec![CachePlan {
            family: "sources".to_owned(),
            root: None,
            restore_step: "restore".to_owned(),
            save_step: "save".to_owned(),
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
            verdict.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }
}
