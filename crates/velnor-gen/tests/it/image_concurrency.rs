// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered};

#[test]
fn a_newer_push_cancels_unfinished_build_and_gate_work_but_never_a_publish() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let mut cancelling = Vec::new();
        let mut pushing = Vec::new();
        for (id, job) in jobs(&workflow)? {
            let concurrency = &job["concurrency"];
            let group = concurrency["group"].as_str().unwrap_or_default();
            if id.ends_with("-build") || id == "image-gate" {
                assert_eq!(concurrency["cancel-in-progress"], true, "{name} {id}");
                assert!(group.contains("github.ref"), "{name} {id}: {group}");
                cancelling.push(group.to_owned());
            } else if id.ends_with("-publish") || id.ends_with("-promote") {
                assert_eq!(concurrency["cancel-in-progress"], false, "{name} {id}");
                assert!(group.contains("github.ref"), "{name} {id}: {group}");
                pushing.push(group.to_owned());
            } else {
                assert!(job.get("concurrency").is_none(), "{name} {id}");
            }
        }
        assert!(!cancelling.is_empty() && !pushing.is_empty(), "{name}");
        for group in &pushing {
            assert!(!cancelling.contains(group), "{name}: {group}");
        }
    }
    Ok(())
}

#[test]
fn a_push_run_never_joins_the_workflow_group_that_cancels_pull_request_runs() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let group = workflow["concurrency"]["group"].to_string();
        let cancel = workflow["concurrency"]["cancel-in-progress"].to_string();
        assert!(
            group.contains("github.event_name == 'push' && format('push-{0}', github.run_id)"),
            "{name}: {group}"
        );
        assert!(
            cancel.contains("github.event_name == 'pull_request'"),
            "{name}: {cancel}"
        );
    }
    Ok(())
}
