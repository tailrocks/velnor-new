// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[test]
fn publisher_emits_one_context_from_selected_producer_after_exact_assets()
-> Result<(), Box<dyn Error>> {
    let document = super::image_workflow(&super::rendered()?)?;
    let jobs = document["jobs"].as_object().ok_or("jobs")?;
    let steps = jobs["publish-images"]["steps"]
        .as_array()
        .ok_or("publisher steps")?;
    let assets = steps
        .iter()
        .position(|step| step["id"] == "assets")
        .ok_or("assets")?;
    let record = steps
        .iter()
        .position(|step| step["name"] == "Record publication context")
        .ok_or("record")?;
    let upload = steps
        .iter()
        .position(|step| step["id"] == "context")
        .ok_or("upload")?;
    assert!(assets < record && record < upload);
    let env = &steps[record]["env"];
    for (name, expression) in [
        (
            "PRODUCER_RUN_ID",
            "${{ needs.discover-producer.outputs.producer_run_id }}",
        ),
        (
            "PRODUCER_RUN_ATTEMPT",
            "${{ needs.discover-producer.outputs.producer_run_attempt }}",
        ),
        (
            "PRODUCER_ARTIFACT_ID",
            "${{ needs.discover-producer.outputs.producer_artifact_id }}",
        ),
        (
            "PUBLISHER_ARTIFACT_ID",
            "${{ steps.assets.outputs.artifact-id }}",
        ),
        (
            "PUBLISHER_DIGEST",
            "${{ steps.assets.outputs.artifact-digest }}",
        ),
    ] {
        assert_eq!(env[name], expression);
    }
    let script = steps[record]["run"].as_str().ok_or("context script")?;
    for needle in [
        "GITHUB_REPOSITORY_ID",
        "GITHUB_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "RUNNER_IMAGE_MANIFEST.json",
        "runner-family-authenticated/runner.lock",
    ] {
        assert!(script.contains(needle), "{needle}");
    }
    assert_eq!(
        steps[upload]["with"]["name"],
        "runner-image-publication-context-${{ github.run_attempt }}"
    );
    assert_eq!(
        steps[upload]["if"],
        "${{ steps.reconcile.outputs.action == 'build' }}"
    );
    let mut matches = 0;
    for job in jobs.values() {
        if let Some(steps) = job["steps"].as_array() {
            matches += steps
                .iter()
                .filter(|step| {
                    step["with"]["name"]
                        .as_str()
                        .is_some_and(|name| name.starts_with("runner-image-publication-context-"))
                })
                .count();
        }
    }
    assert_eq!(matches, 1);
    Ok(())
}
