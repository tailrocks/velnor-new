// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered, step_named, steps};

#[test]
fn only_the_advance_job_writes_the_branch_tag_by_digest_through_the_helper_without_repository_code()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let mut advances = 0;
        for (id, job) in jobs(&workflow)? {
            if let Some(image) = id
                .strip_prefix("image-")
                .and_then(|rest| rest.strip_suffix("-publish"))
            {
                assert_eq!(
                    job["outputs"]["digest"], "${{ steps.push.outputs.digest }}",
                    "{name} {id}"
                );
                let push = step_named(job, "Build and push")?;
                for line in push["with"]["tags"].as_str().unwrap_or_default().lines() {
                    assert!(
                        line.ends_with(":sha-${{ github.sha }}"),
                        "{name} {id}: {line}"
                    );
                }
                assert!(
                    !steps(job).iter().any(|step| step["id"] == "tip"),
                    "{name} {id}"
                );
                let advance = &workflow["jobs"][format!("image-{image}-advance")];
                assert_eq!(advance["needs"], serde_json::json!([id]), "{name} {id}");
                advances += 1;
            }
            if id.ends_with("-advance") {
                assert_eq!(job["environment"], "images", "{name} {id}");
                assert_eq!(
                    job["concurrency"]["cancel-in-progress"], false,
                    "{name} {id}"
                );
                let group = job["concurrency"]["group"].as_str().unwrap_or_default();
                assert!(
                    group.starts_with(id) && group.ends_with("${{ github.ref }}"),
                    "{name} {id}: {group}"
                );
                let ordered = steps(job);
                let moves: Vec<&&serde_json::Value> = ordered
                    .iter()
                    .filter(|step| {
                        step["run"]
                            .as_str()
                            .is_some_and(|run| run.contains("image advance"))
                    })
                    .collect();
                assert_eq!(moves.len(), 1, "{name} {id}");
                assert!(
                    moves[0]["env"]["DIGEST"]
                        .as_str()
                        .is_some_and(|text| text.contains(".outputs.digest")),
                    "{name} {id}"
                );
                assert!(
                    ordered.iter().all(|step| {
                        step["run"]
                            .as_str()
                            .is_none_or(|run| !run.contains("imagetools"))
                    }),
                    "{name} {id}: the helper is the only writer of the branch tag"
                );
                assert!(
                    ordered.iter().all(|step| {
                        !step["uses"]
                            .as_str()
                            .is_some_and(|uses| uses.starts_with("actions/checkout"))
                            && step["run"]
                                .as_str()
                                .is_none_or(|run| !run.contains("./") && !run.contains("cargo"))
                    }),
                    "{name} {id}: the advance job checks out nothing and runs no repository code"
                );
            }
        }
        assert!(advances > 0, "{name}");
    }
    Ok(())
}

#[test]
fn images_never_share_an_advance_lock() -> TestResult {
    let workflow = rendered("image-workspace")?;
    let mut groups: Vec<String> = jobs(&workflow)?
        .iter()
        .filter(|(id, _)| id.ends_with("-advance"))
        .map(|(_, job)| job["concurrency"]["group"].to_string())
        .collect();
    assert_eq!(groups.len(), 2);
    groups.dedup();
    assert_eq!(groups.len(), 2);
    Ok(())
}

#[test]
fn a_dropped_advance_heals_because_every_advance_points_the_branch_at_the_tip() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !id.ends_with("-advance") {
                continue;
            }
            let run =
                step_named(job, "Point the default-branch tag at the image of the tip")?["run"]
                    .as_str()
                    .ok_or("no run")?
                    .to_owned();
            assert!(run.contains("--commit \"$COMMIT\""), "{name} {id}: {run}");
            assert!(run.contains("--image="), "{name} {id}: {run}");
            assert!(
                step_named(job, "Say that the default-branch tag was withheld")?["if"]
                    .as_str()
                    .is_some_and(|text| text.contains("steps.advance.outputs.moved != 'true'")),
                "{name} {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_build_goes_live_only_when_the_tip_was_checked_again_right_before_the_compile() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !id.ends_with("-build") {
                continue;
            }
            let live = job["outputs"]["live"].as_str().ok_or("no live output")?;
            assert!(
                live.contains("steps.tip.outputs.ready == 'true'"),
                "{name} {id}"
            );
            assert!(
                live.contains("steps.recheck.outputs.ready == 'true'"),
                "{name} {id}"
            );
        }
    }
    Ok(())
}
