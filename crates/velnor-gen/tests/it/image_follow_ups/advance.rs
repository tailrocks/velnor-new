// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered, step_named, steps};

#[test]
fn only_the_advance_job_writes_the_branch_tag_and_it_does_so_by_digest_after_a_tip_check()
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
                let tip = ordered
                    .iter()
                    .position(|step| step["id"] == "tip")
                    .ok_or("no tip step")?;
                let creates: Vec<usize> = ordered
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| {
                        step["run"]
                            .as_str()
                            .is_some_and(|run| run.contains("imagetools create"))
                    })
                    .map(|(index, _)| index)
                    .collect();
                assert!(!creates.is_empty(), "{name} {id}");
                for create in creates {
                    assert!(tip < create, "{name} {id}");
                    let step = &ordered[create];
                    assert!(
                        step["if"]
                            .as_str()
                            .is_some_and(|text| text.contains("steps.tip.outputs.ready == 'true'")),
                        "{name} {id}"
                    );
                    assert_eq!(
                        step["run"],
                        "docker buildx imagetools create --tag \"$IMAGE:$BRANCH\" \"$IMAGE@$DIGEST\"",
                        "{name} {id}"
                    );
                    assert!(
                        step["env"]["DIGEST"]
                            .as_str()
                            .is_some_and(|text| text.contains(".outputs.digest")),
                        "{name} {id}"
                    );
                }
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
