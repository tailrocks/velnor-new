// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::tools_verify::{STORES, jobs_of, repo_for, text};

const SOURCES_SAVED_KEY_STEP: &str = "sources_";
const TARGET_SAVED_KEY_STEP: &str = "target_key";

struct Trimmed {
    family: String,
    save_key_step: Option<String>,
    record_ids: Vec<String>,
}

fn trimmed_caches(config: &str, helper: &str) -> Result<Vec<Trimmed>, Box<dyn std::error::Error>> {
    let mut caches = Vec::new();
    for (_id, steps) in jobs_of(&crate::pipeline::workflow(&repo_for(config, helper)?)?)? {
        let Some(report) = steps
            .iter()
            .find(|step| step["name"] == "Report cache telemetry")
        else {
            continue;
        };
        let plan: serde_json::Value =
            serde_json::from_str(&text(&report["env"], "VELNOR_REPORT_CACHES"))?;
        let record_ids: Vec<String> = steps
            .iter()
            .filter(|step| {
                text(step, "name").starts_with("Record the ")
                    && text(step, "name").ends_with(" cache key")
            })
            .filter_map(|step| step["id"].as_str().map(str::to_owned))
            .collect();
        for cache in plan.as_array().into_iter().flatten() {
            let family = cache["family"].as_str().unwrap_or_default().to_owned();
            if family != "target" && family != "sources" {
                continue;
            }
            caches.push(Trimmed {
                family,
                save_key_step: cache["save_key_step"].as_str().map(str::to_owned),
                record_ids: record_ids.clone(),
            });
        }
    }
    Ok(caches)
}

#[test]
fn a_target_and_sources_save_reports_the_key_it_recorded_from_helper_0_1_112() -> TestResult {
    for config in STORES.iter().copied() {
        let caches = trimmed_caches(config, "0.1.112")?;
        assert!(
            caches.iter().any(|cache| cache.family == "target"),
            "{config:?}: no target cache in the report"
        );
        assert!(
            caches.iter().any(|cache| cache.family == "sources"),
            "{config:?}: no sources cache in the report"
        );
        for cache in caches {
            let expected = if cache.family == "target" {
                TARGET_SAVED_KEY_STEP
            } else {
                SOURCES_SAVED_KEY_STEP
            };
            let step = cache
                .save_key_step
                .as_deref()
                .ok_or_else(|| format!("{config:?} {}: no save_key_step", cache.family))?;
            assert!(
                step.starts_with(expected) && step.ends_with("_key"),
                "{config:?} {}: {step}",
                cache.family
            );
            assert!(
                cache.record_ids.iter().any(|id| id == step),
                "{config:?} {}: {step} is not a recorded key step {:?}",
                cache.family,
                cache.record_ids
            );
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_gets_no_saved_key_step_for_target_or_sources() -> TestResult {
    for config in STORES.iter().copied() {
        let caches = trimmed_caches(config, "0.1.110")?;
        assert!(!caches.is_empty(), "{config:?}: no target or sources cache");
        for cache in caches {
            assert!(
                cache.save_key_step.is_none(),
                "{config:?} {}: {:?}",
                cache.family,
                cache.save_key_step
            );
        }
    }
    Ok(())
}
