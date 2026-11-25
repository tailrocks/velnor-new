// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, jobs, rendered, steps};

#[test]
fn a_required_image_job_fails_when_a_job_it_needs_failed_instead_of_being_skipped() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if !id.ends_with("-check") {
                continue;
            }
            let condition = job["if"].as_str().ok_or("no condition")?;
            assert!(
                condition.contains("!cancelled()"),
                "{name} {id}: {condition}"
            );
            let first = steps(job).first().copied().ok_or("no step")?;
            assert_eq!(
                first["name"], "Fail because a job that this check needs failed",
                "{name} {id}"
            );
            assert_eq!(first["run"], "false", "{name} {id}");
            assert!(
                first["if"]
                    .as_str()
                    .is_some_and(|text| text.contains(".result == 'failure'")),
                "{name} {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_build_that_needs_the_image_selection_fails_when_the_selection_failed() -> TestResult {
    let workflow = rendered("image-ghcr")?;
    let build = &workflow["jobs"]["image-cluster-svc-build"];
    let condition = build["if"].as_str().ok_or("no condition")?;
    assert!(condition.contains("!cancelled()"), "{condition}");
    assert!(
        condition.contains("needs.image-changes.result == 'failure'"),
        "{condition}"
    );
    assert!(steps(build).iter().any(|step| {
        step["name"] == "Fail because the image selection failed" && step["run"] == "false"
    }));
    Ok(())
}
