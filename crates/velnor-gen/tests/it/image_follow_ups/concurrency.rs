// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered, step_named, steps};

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
fn a_release_tag_on_a_commit_without_an_image_fails_with_a_message_naming_the_rerun() -> TestResult
{
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !id.ends_with("-promote") {
                continue;
            }
            let ordered = steps(job);
            let first_create = ordered
                .iter()
                .position(|step| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("imagetools create"))
                })
                .ok_or("no promote")?;
            let looks: Vec<usize> = ordered
                .iter()
                .enumerate()
                .filter(|(_, step)| {
                    step["run"]
                        .as_str()
                        .is_some_and(|run| run.contains("imagetools inspect"))
                })
                .map(|(index, _)| index)
                .collect();
            assert!(
                !looks.is_empty() && looks.iter().all(|look| *look < first_create),
                "{name} {id}"
            );
            for look in looks {
                assert_eq!(ordered[look]["continue-on-error"], true, "{name} {id}");
            }
            let explain = step_named(job, "Explain the missing commit image")?;
            assert!(
                explain["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("Re-run all jobs")),
                "{name} {id}"
            );
            assert!(
                explain["if"]
                    .as_str()
                    .is_some_and(|run| run.contains("outcome == 'failure'")),
                "{name} {id}"
            );
            assert_eq!(step_named(job, "Stop: the commit image")?["run"], "exit 1");
            let tell = step_named(job, "Tell a missing image from a registry error")?;
            assert!(
                tell["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("manifest unknown")),
                "{name} {id}"
            );
            assert!(
                explain["if"]
                    .as_str()
                    .is_some_and(|text| text.contains("miss-0.outcome == 'success'")),
                "{name} {id}"
            );
        }
    }
    Ok(())
}
