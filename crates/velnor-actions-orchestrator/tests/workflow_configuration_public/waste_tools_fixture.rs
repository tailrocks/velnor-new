// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{lane_jobs, lane_jobs_for_runner};
use std::error::Error;
use velnor_actions_contract::StepKind;

#[test]
fn gh_fixture_test_jobs_require_isolated_tool_staging() -> Result<(), Box<dyn Error>> {
    let jobs = lane_jobs()?;
    for job in jobs
        .values()
        .filter(|job| job.display_name.contains("velnor-actions-orchestrator"))
    {
        assert!(
            job.steps
                .iter()
                .any(|step| step.name == "Stage isolated GitHub CLI test tools")
        );
    }
    let job = jobs
        .values()
        .find(|job| job.display_name.contains("velnor-actions-cli"))
        .ok_or("CLI job")?;
    let stage = job
        .steps
        .iter()
        .position(|step| step.name == "Stage isolated GitHub CLI test tools")
        .ok_or("missing fixture staging")?;
    let fetch = job
        .steps
        .iter()
        .position(|step| step.name.contains("Fetch") && step.name.contains("source"))
        .ok_or("missing source fetch")?;
    assert!(stage > fetch);
    let StepKind::Shell { run, .. } = &job.steps[stage].kind else {
        return Err("fixture staging must run".into());
    };
    assert!(run.ends_with(&[
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "scripts/prepare-gh-fixture-tools.py".to_owned(),
        "--with-image-tuf".to_owned(),
    ]));
    Ok(())
}

#[test]
fn cargo_test_jobs_stage_image_tuf_for_integration_tests() -> Result<(), Box<dyn Error>> {
    let (jobs, _) = lane_jobs_for_runner(true)?;
    let cli = jobs
        .values()
        .find(|job| job.display_name.contains("velnor-actions-cli"))
        .ok_or("CLI job")?;
    let stage = cli
        .steps
        .iter()
        .find(|step| step.name == "Stage isolated GitHub CLI test tools")
        .ok_or("CLI staging")?;
    let StepKind::Shell { run, .. } = &stage.kind else {
        return Err("fixture staging must run".into());
    };
    assert!(run.iter().any(|arg| arg == "--with-image-tuf"));
    let orchestrator = jobs
        .values()
        .find(|job| job.display_name.contains("velnor-actions-orchestrator"))
        .ok_or("orchestrator job")?;
    let stage = orchestrator
        .steps
        .iter()
        .find(|step| step.name == "Stage isolated GitHub CLI test tools")
        .ok_or("orchestrator staging")?;
    let StepKind::Shell { run, .. } = &stage.kind else {
        return Err("fixture staging must run".into());
    };
    assert!(!run.iter().any(|arg| arg == "--with-image-tuf"));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn gh_fixture_staging_rejects_unverified_tools_and_missing_docker() -> Result<(), Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace root")?;
    let status = std::process::Command::new("python3")
        .arg("-I")
        .arg(root.join("scripts/test-prepare-gh-fixture-tools.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}
