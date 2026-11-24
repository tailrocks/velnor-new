// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, edited, image, jobs, name_of, rendered, step_named};

const BOT: &str = "!endsWith(github.actor, '[bot]')";
const QUEUE: &str = "github.event_name == 'merge_group'";

#[test]
fn the_merge_queue_commit_is_built_and_checked_but_never_published() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        assert_eq!(
            workflow["on"]["merge_group"]["types"],
            serde_json::json!(["checks_requested"]),
            "{name}"
        );
        for (id, job) in jobs(&workflow)? {
            let condition = job["if"].as_str().unwrap_or_default();
            let checked = id.ends_with("-build") || id.ends_with("-check");
            assert_eq!(condition.contains(QUEUE), checked, "{name} {id}");
            if checked {
                assert!(
                    condition.contains(&format!("github.repository == '{name}'")),
                    "{name} {id}"
                );
                assert!(job.get("environment").is_none(), "{name} {id}");
            }
        }
    }
    Ok(())
}

#[test]
fn an_image_that_is_not_built_for_pull_requests_is_not_built_in_the_merge_queue() -> TestResult {
    let view = edited("image-plain", |text| {
        text.replace("pr_build = \"always\"", "pr_build = \"off\"")
    })?;
    let workflow = image(&view)?;
    assert!(workflow["on"].get("merge_group").is_none());
    assert!(workflow["on"].get("pull_request").is_none());
    for (id, job) in jobs(&workflow)? {
        assert!(
            !job["if"].as_str().unwrap_or_default().contains(QUEUE),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn a_bot_pull_request_is_checked_when_no_private_token_is_involved() -> TestResult {
    for name in ["image-plain", "image-workspace"] {
        let workflow = rendered(name)?;
        for part in ["build", "check"] {
            let job = &workflow["jobs"][format!("image-{}-{part}", name_of(name))];
            let condition = job["if"].as_str().ok_or("no condition")?;
            assert!(!condition.contains("[bot]"), "{name} {part}");
            assert!(
                condition
                    .contains("github.event.pull_request.head.repo.full_name == github.repository"),
                "{name} {part}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_bot_pull_request_skips_the_image_jobs_that_need_the_private_token_and_the_queue_runs_them()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    for part in ["build", "check"] {
        let job = &workflow["jobs"][format!("image-{}-{part}", name_of("image-ghcr"))];
        let condition = job["if"].as_str().ok_or("no condition")?;
        assert!(condition.contains(BOT), "{part}");
        assert!(condition.contains(QUEUE), "{part}");
    }
    let build = &workflow["jobs"]["image-cluster-svc-build"];
    let check = step_named(build, "merge queue entry is not from a fork")?;
    assert_eq!(check["if"], "${{ github.event_name == 'merge_group' }}");
    let token = step_named(build, "read-only token for the private repositories")?;
    let condition = token["if"].as_str().ok_or("no token condition")?;
    assert!(
        condition.contains("steps.private-check.outputs.allowed == 'true'"),
        "{condition}"
    );
    Ok(())
}
