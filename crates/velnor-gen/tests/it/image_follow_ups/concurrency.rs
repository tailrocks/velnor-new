// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered, steps};

fn position(job: &Value, name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    steps(job)
        .iter()
        .position(|step| {
            step["name"]
                .as_str()
                .is_some_and(|text| text.contains(name))
        })
        .ok_or_else(|| format!("no step named {name}").into())
}

#[test]
fn a_run_of_an_older_commit_never_shares_a_concurrency_group_with_the_newest_commit() -> TestResult
{
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let mut seen = 0;
        for (id, job) in jobs(&workflow)? {
            let group = job["concurrency"]["group"].as_str().unwrap_or_default();
            if id.ends_with("-build") || id == "image-gate" || id.ends_with("-publish") {
                seen += 1;
                assert!(group.contains("github.sha"), "{name} {id}: {group}");
                assert!(!group.contains("github.ref"), "{name} {id}: {group}");
            }
            if id.ends_with("-publish") {
                assert_eq!(
                    job["concurrency"]["cancel-in-progress"], false,
                    "{name} {id}"
                );
            }
            if id.ends_with("-promote") {
                assert_eq!(
                    job["concurrency"]["cancel-in-progress"], false,
                    "{name} {id}"
                );
            }
        }
        assert!(seen >= 3, "{name}");
        let group = workflow["concurrency"]["group"].to_string();
        assert!(
            group.contains("github.event_name == 'push' && format('push-{0}', github.run_id)"),
            "{name}: {group}"
        );
    }
    Ok(())
}

#[test]
fn a_build_or_gate_of_a_commit_that_is_not_the_tip_stands_down_before_any_expensive_step()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !(id.ends_with("-build") || id == "image-gate") {
                continue;
            }
            let tip = position(job, "still the tip")?;
            let ordered = steps(job);
            assert_eq!(ordered[tip]["id"], "tip", "{name} {id}");
            let condition = ordered[tip]["if"].as_str().unwrap_or_default();
            assert!(condition.contains("github.run_attempt == 1"), "{name} {id}");
            let expensive = position(
                job,
                if id == "image-gate" {
                    "Wait for"
                } else {
                    "Compile"
                },
            )?;
            assert!(tip < expensive, "{name} {id}");
            if id.ends_with("-build") {
                let again = ordered
                    .iter()
                    .position(|step| step["id"] == "recheck")
                    .ok_or("no recheck")?;
                assert!(again < expensive, "{name} {id}");
                for step in &ordered[expensive..] {
                    let condition = step["if"].as_str().unwrap_or_default();
                    assert!(
                        condition.contains("steps.recheck.outputs.ready == 'true'"),
                        "{name} {id}: {}",
                        step["name"]
                    );
                }
            }
            for step in &ordered[tip + 1..] {
                let condition = step["if"].as_str().unwrap_or_default();
                assert!(
                    condition.contains("steps.tip.outputs.ready == 'true'"),
                    "{name} {id}: {}",
                    step["name"]
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_publish_waits_for_a_build_that_did_not_stand_down() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if id.ends_with("-publish") {
                let condition = job["if"].as_str().unwrap_or_default();
                assert!(condition.contains("outputs.live == 'true'"), "{name} {id}");
            }
            if id.ends_with("-build") {
                let live = job["outputs"]["live"].as_str().unwrap_or_default();
                assert!(live.contains("github.run_attempt != 1"), "{name} {id}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_release_tag_is_promoted_only_by_the_helper_that_names_the_rerun_for_a_missing_image()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !id.ends_with("-promote") {
                continue;
            }
            let ordered = steps(job);
            let promote = ordered
                .iter()
                .filter(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("image promote"))
                })
                .count();
            assert_eq!(promote, 1, "{name} {id}");
            assert!(
                ordered.iter().all(|step| {
                    step["run"]
                        .as_str()
                        .is_none_or(|run| !run.contains("imagetools") && !run.contains("grep"))
                }),
                "{name} {id}: the helper is the only path that reads or writes a registry tag"
            );
        }
    }
    Ok(())
}
