// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::tools_verify::{STORES, jobs_of, repo_for, text};

const RECORD_NAME: &str = "Record the tools cache key";
const SAVED_KEY_STEP: &str = "tools_saved_key";

struct SavingJob {
    id: String,
    saves: usize,
    records: Vec<serde_json::Value>,
    report_plan: serde_json::Value,
    save_position: usize,
    record_position: Option<usize>,
}

fn saving_jobs(config: &str, helper: &str) -> Result<Vec<SavingJob>, Box<dyn std::error::Error>> {
    let mut jobs = Vec::new();
    for (id, steps) in jobs_of(&workflow(&repo_for(config, helper)?)?)? {
        let saves: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                text(step, "uses").starts_with("actions/cache/save@")
                    && step["with"]["key"]
                        .as_str()
                        .is_some_and(|key| key.starts_with("velnor-tools-"))
            })
            .map(|(position, _)| position)
            .collect();
        let Some(&save_position) = saves.last() else {
            continue;
        };
        let report = steps
            .iter()
            .find(|step| step["name"] == "Report cache telemetry")
            .ok_or_else(|| format!("{config:?} {id}: no report step"))?;
        let plan: serde_json::Value =
            serde_json::from_str(&text(&report["env"], "VELNOR_REPORT_CACHES"))?;
        let report_plan = plan
            .as_array()
            .and_then(|caches| caches.iter().find(|cache| cache["family"] == "tools"))
            .cloned()
            .ok_or_else(|| format!("{config:?} {id}: no tools cache in the report"))?;
        jobs.push(SavingJob {
            id,
            saves: saves.len(),
            records: steps
                .iter()
                .filter(|step| step["name"] == RECORD_NAME)
                .cloned()
                .collect(),
            report_plan,
            save_position,
            record_position: steps.iter().position(|step| step["name"] == RECORD_NAME),
        });
    }
    Ok(jobs)
}

#[test]
fn the_tools_save_exposes_its_recorded_key_and_the_report_reads_it_from_helper_0_1_112()
-> TestResult {
    for config in STORES.iter().copied() {
        let jobs = saving_jobs(config, "0.1.112")?;
        assert_ne!(jobs.len(), 0, "{config:?}: no job saves the tools");
        for job in jobs {
            let SavingJob {
                id,
                saves,
                records,
                report_plan,
                save_position,
                record_position,
            } = job;
            assert!(saves >= 1, "{config:?} {id}");
            assert_eq!(records.len(), 1, "{config:?} {id}");
            let record = &records[0];
            assert_eq!(record["id"], SAVED_KEY_STEP, "{config:?} {id}");
            assert!(
                record_position.is_some_and(|position| position > save_position),
                "{config:?} {id}: the key is recorded before the save"
            );
            assert_eq!(
                record["env"]["CACHE_KEY"].as_str().map(|key| key
                    .ends_with("-${{ steps.toolsrecord.outputs.digest }}")
                    && key.starts_with("velnor-tools-")),
                Some(true),
                "{config:?} {id}: {record}"
            );
            assert_eq!(
                record["if"], "${{ steps.save_tools.outcome == 'success' }}",
                "{config:?} {id}"
            );
            assert!(
                text(record, "run").ends_with("key=$CACHE_KEY\" >> \"$GITHUB_OUTPUT\""),
                "{config:?} {id}"
            );
            assert_eq!(
                report_plan["save_key_step"], SAVED_KEY_STEP,
                "{config:?} {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_gets_no_saved_key_step_and_no_report_field() -> TestResult {
    for helper in ["0.1.110", "0.1.111"] {
        for config in STORES.iter().copied() {
            let jobs = saving_jobs(config, helper)?;
            assert_ne!(jobs.len(), 0, "{config:?} {helper}: no job saves the tools");
            for job in jobs {
                assert!(job.records.is_empty(), "{config:?} {helper} {}", job.id);
                assert!(
                    job.report_plan.get("save_key_step").is_none(),
                    "{config:?} {helper} {}",
                    job.id
                );
            }
        }
    }
    Ok(())
}
