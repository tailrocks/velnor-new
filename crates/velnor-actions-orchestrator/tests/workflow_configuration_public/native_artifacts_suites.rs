// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{Job, REQUIRED_JOB_ID, StepKind, StepRole};
use velnor_actions_orchestrator::{GenerationPreparation, finalized_jobs, render_staged_tree};
use velnor_actions_workflow_renderer::{
    CompileDriver, render::WORKFLOW_PATH, steps::check_mbx_gating,
};

use super::native_artifacts_fixture::repo_with_rust_suite;
use super::support::rendered_job_block;

const SUITE_ID: &str = "suite-proof-suite";
const BUILD_ID: &str = "suite-build-proof-suite";

#[test]
fn public_rust_suite_jobs_keep_their_effective_compile_driver() -> Result<(), Box<dyn Error>> {
    for (driver, expected_driver) in [("cargo", "cargo"), ("mbx", "mbx")] {
        for (mode, partitions, build) in [
            ("single", 1, "inline"),
            ("inline", 2, "inline"),
            ("archive", 2, "archive"),
        ] {
            assert_suite_jobs(driver, expected_driver, mode, partitions, build)?;
        }
    }
    Ok(())
}

fn assert_suite_jobs(
    configured: &str,
    expected_driver: &str,
    mode: &str,
    partitions: u16,
    build: &str,
) -> Result<(), Box<dyn Error>> {
    let (_repo, prepared) = repo_with_rust_suite(configured, partitions, build)?;
    assert_gate_accepts(&prepared, configured, mode)?;
    let jobs = finalized_jobs(&prepared)?;
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("Required job is missing")?;
    let suite = jobs.get(SUITE_ID).ok_or("suite job is missing")?;
    assert!(required.needs.iter().any(|job| job == SUITE_ID));
    let run_driver = if mode == "archive" {
        "cargo"
    } else {
        expected_driver
    };
    let run = shell_command(suite, "Run suite (proof-suite)")?;
    assert!(
        run.contains(&format!("-- {run_driver} nextest run")),
        "{run}"
    );
    assert_eq!(has_mbx_preflight(suite), run_driver == "mbx");
    assert_suite_upload(suite)?;

    let tree = render_staged_tree(&prepared)?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("generated CI is missing")?;
    let suite_block = rendered_job_block(workflow, SUITE_ID)?;
    assert!(suite_block.contains("if: failure()"));
    assert!(suite_block.contains("if-no-files-found: warn"));
    assert!(!suite_block.contains("include-hidden-files"));
    assert!(suite_block.contains(&format!("-- {run_driver} nextest run")));

    if mode == "archive" {
        assert!(required.needs.iter().any(|job| job == BUILD_ID));
        let build_job = jobs.get(BUILD_ID).ok_or("archive build job is missing")?;
        assert!(suite.needs.iter().any(|job| job == BUILD_ID));
        let archive = shell_command(build_job, "Build suite (proof-suite)")?;
        assert!(archive.contains(&format!("-- {expected_driver} nextest archive")));
        assert_eq!(has_mbx_preflight(build_job), expected_driver == "mbx");
        assert!(!has_mbx_preflight(suite));
        let build_block = rendered_job_block(workflow, BUILD_ID)?;
        assert!(build_block.contains(&format!("-- {expected_driver} nextest archive")));
    }
    Ok(())
}

fn suite_job(
    jobs: &BTreeMap<String, Job>,
    id: &str,
) -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    Ok(suite_job_from_job(
        id,
        jobs.get(id).ok_or("suite job is missing")?.clone(),
    ))
}

fn suite_job_from_job(id: &str, job: Job) -> BTreeMap<String, Job> {
    BTreeMap::from([(id.to_owned(), job)])
}

fn assert_gate_accepts(
    prepared: &GenerationPreparation,
    configured: &str,
    mode: &str,
) -> Result<(), Box<dyn Error>> {
    let selected = configured_driver(configured);
    let run_driver = if mode == "archive" {
        CompileDriver::Cargo
    } else {
        selected
    };
    let mut jobs = suite_job(&prepared.workflow.ir.jobs, SUITE_ID)?;
    let mut drivers = BTreeMap::from([(SUITE_ID.to_owned(), run_driver)]);
    if mode == "archive" {
        jobs.extend(suite_job(&prepared.workflow.ir.jobs, BUILD_ID)?);
        drivers.insert(BUILD_ID.to_owned(), selected);
    }
    check_mbx_gating(&jobs, &drivers).map_err(Into::into)
}

fn configured_driver(configured: &str) -> CompileDriver {
    match configured {
        "cargo" => CompileDriver::Cargo,
        "mbx" => CompileDriver::Mbx,
        _ => unreachable!("fixture only uses known compile drivers"),
    }
}

fn shell_command(job: &Job, name: &str) -> Result<String, Box<dyn Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or_else(|| format!("shell step {name} is missing"))?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(format!("step {name} is not a shell command").into());
    };
    Ok(run.join(" "))
}

fn has_mbx_preflight(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::MbxPreflight))
}

fn assert_suite_upload(job: &Job) -> Result<(), Box<dyn Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == "Upload suite artifact (suite-evidence)")
        .ok_or("suite artifact upload is missing")?;
    assert_eq!(step.condition.as_deref(), Some("failure()"));
    let StepKind::Action { with, .. } = &step.kind else {
        return Err("suite artifact upload is not an action".into());
    };
    assert_eq!(
        with.get("if-no-files-found").map(String::as_str),
        Some("warn")
    );
    assert_eq!(with.get("retention-days").map(String::as_str), Some("7"));
    assert!(!with.contains_key("include-hidden-files"));
    Ok(())
}
