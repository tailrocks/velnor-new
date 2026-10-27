// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::ids::{
    matrix_id_for_task_group, matrix_key_for_id, task_id_for_internal,
};
use velnor_actions_contract::workflow::crate_job::{
    CrateGroupPlan, CrateJob, CrateObligation, GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX,
    MAX_GROUP_UNITS, SEED_JOB_ID_PREFIX, group_timeout_for, is_seed_job_id, reader_job_id_for,
    task_digest_for_execution, validate_group_needs, writer_job_id_for,
};
use velnor_actions_contract::workflow::jobs::crate_display_name;

const KINDS: [(&str, &[&str]); 3] = [
    ("build", &["mbx", "build", "--locked"]),
    ("clippy", &["mbx", "clippy", "--locked", "--all-targets"]),
    ("doc", &["mbx", "doc", "--locked", "--no-deps"]),
];

fn obligation(kind: &str, run: &[&str]) -> Result<CrateObligation, Box<dyn Error>> {
    let task_id = task_id_for_internal(kind, "dev")?;
    let matrix_id = matrix_id_for_task_group("rust", task_id.as_str())?;
    let inputs = ToolchainInputs {
        tools: vec!["rust".to_owned()],
        components: Vec::new(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo".to_owned(),
    };
    let toolchain = toolchain_id(&inputs)?;
    let argv: Vec<String> = run.iter().map(|arg| (*arg).to_owned()).collect();
    let task_digest = task_digest_for_execution(task_id.as_str(), &argv, &toolchain)?;
    Ok(CrateObligation {
        task_id,
        kind: kind.to_owned(),
        step_name: format!("Run {kind} lane"),
        gated_by: Vec::new(),
        matrix_key: matrix_key_for_id(&matrix_id)?,
        task_digest,
        toolchain_inputs: inputs,
        run: argv,
    })
}

fn fixture_job() -> Result<CrateJob, Box<dyn Error>> {
    let mut obligations = Vec::with_capacity(KINDS.len());
    for (kind, run) in KINDS {
        obligations.push(obligation(kind, run)?);
    }
    let job = CrateJob {
        job_id: "rust-demo".to_owned(),
        display_name: crate_display_name("demo", "crates/demo/Cargo.toml", "default"),
        package_name: "demo".to_owned(),
        package_id: "demo".to_owned(),
        manifest: "crates/demo/Cargo.toml".to_owned(),
        configuration: "default".to_owned(),
        obligations,
    };
    job.validate()?;
    Ok(job)
}

fn fixture_plan(job: &CrateJob, bound: JobTimeout) -> Result<CrateGroupPlan, Box<dyn Error>> {
    let mut unit_ids: Vec<String> = job
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    unit_ids.sort();
    Ok(CrateGroupPlan::new(
        job.job_id.clone(),
        String::new(),
        "ubuntu-24.04".to_owned(),
        unit_ids,
        group_timeout_for(None, bound),
    )?)
}

fn check_same_units_and_commands() -> Result<(), Box<dyn Error>> {
    let job = fixture_job()?;
    let plan = fixture_plan(&job, JobTimeout::new(30)?)?;
    plan.validate()?;
    let mut expected: Vec<String> = job
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    expected.sort();
    if plan.unit_ids != expected || plan.group_job_id != job.job_id {
        return Err(io::Error::other("group plan dropped or renamed units").into());
    }
    for (obligation, (_, want)) in job.obligations.iter().zip(KINDS.iter()) {
        let want: Vec<String> = want.iter().map(|arg| (*arg).to_owned()).collect();
        if obligation.run != want {
            return Err(io::Error::other("group plan altered exact package argv").into());
        }
    }
    Ok(())
}

fn check_static_reader_writer() -> Result<(), Box<dyn Error>> {
    let job = fixture_job()?;
    let plan = fixture_plan(&job, JobTimeout::new(30)?)?;
    if plan.reader_job_id() != format!("{}{GROUP_READER_SUFFIX}", job.job_id)
        || plan.writer_job_id() != format!("{}{GROUP_WRITER_SUFFIX}", job.job_id)
        || reader_job_id_for(&job.job_id) != plan.reader_job_id()
        || writer_job_id_for(&job.job_id) != plan.writer_job_id()
        || plan.reader_job_id() == plan.writer_job_id()
    {
        return Err(io::Error::other("static branch ids drifted").into());
    }
    validate_group_needs(&["plan".to_owned(), job.job_id.clone()], &job.job_id)?;
    Ok(())
}

fn check_grouped_deadlines() -> Result<(), Box<dyn Error>> {
    let bound = JobTimeout::new(30)?;
    if group_timeout_for(None, bound).minutes() != 30
        || group_timeout_for(Some(7), bound).minutes() != 7
        || group_timeout_for(Some(900), bound).minutes() != 30
        || group_timeout_for(Some(0), bound).minutes() != JobTimeout::MIN_MINUTES
    {
        return Err(io::Error::other("grouped deadline derivation drifted").into());
    }
    let plan = fixture_plan(&fixture_job()?, bound)?;
    if plan.timeout_minutes.minutes() != 30 {
        return Err(io::Error::other("group deadline escaped its bound").into());
    }
    Ok(())
}

#[test]
fn gc1_t08_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_same_units_and_commands()?;
    check_static_reader_writer()?;
    check_grouped_deadlines()?;
    if !is_seed_job_id(SEED_JOB_ID_PREFIX)
        || !is_seed_job_id("dependency-seed-workspace")
        || is_seed_job_id("rust-demo")
    {
        return Err(io::Error::other("seed job guard misclassifies ids").into());
    }
    Ok(())
}

fn reject_case(
    units: Vec<String>,
    label: &str,
    workspace: &str,
    environment: &str,
    group: &str,
    bound: JobTimeout,
) -> Result<(), Box<dyn Error>> {
    if CrateGroupPlan::new(
        group.to_owned(),
        workspace.to_owned(),
        environment.to_owned(),
        units,
        bound,
    )
    .is_ok()
    {
        return Err(io::Error::other(format!("invalid plan accepted: {label}")).into());
    }
    Ok(())
}

fn check_rejects_bad_plans() -> Result<(), Box<dyn Error>> {
    let bound = JobTimeout::new(30)?;
    let job = fixture_job()?;
    let mut unit_ids: Vec<String> = job
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    unit_ids.sort();
    reject_case(unit_ids.clone(), "empty id", "", "ubuntu-24.04", "", bound)?;
    Ok(())
}

fn check_rejects_bad_roots() -> Result<(), Box<dyn Error>> {
    let bound = JobTimeout::new(30)?;
    let job = fixture_job()?;
    let mut unit_ids: Vec<String> = job
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    unit_ids.sort();
    reject_case(
        unit_ids.clone(),
        "absolute root",
        "/absolute/root",
        "ubuntu-24.04",
        &job.job_id,
        bound,
    )?;
    reject_case(
        unit_ids.clone(),
        "parent root",
        "../escape",
        "ubuntu-24.04",
        &job.job_id,
        bound,
    )?;
    reject_case(
        unit_ids.clone(),
        "missing environment",
        "",
        "",
        &job.job_id,
        bound,
    )?;
    reject_case(
        Vec::new(),
        "empty units",
        "",
        "ubuntu-24.04",
        &job.job_id,
        bound,
    )?;
    let mut duplicated = unit_ids.clone();
    duplicated.push(unit_ids[0].clone());
    duplicated.sort();
    reject_case(
        duplicated,
        "duplicate units",
        "",
        "ubuntu-24.04",
        &job.job_id,
        bound,
    )?;
    let mut oversized: Vec<String> = (0..=MAX_GROUP_UNITS)
        .map(|index| format!("internal/kind{index:03}/dev"))
        .collect();
    oversized.sort();
    reject_case(
        oversized,
        "oversized group",
        "",
        "ubuntu-24.04",
        &job.job_id,
        bound,
    )?;
    Ok(())
}

fn check_rejects_seed_race() -> Result<(), Box<dyn Error>> {
    let job = fixture_job()?;
    for seed in [
        SEED_JOB_ID_PREFIX.to_owned(),
        "dependency-seed-workspace".to_owned(),
    ] {
        if validate_group_needs(&["plan".to_owned(), seed.clone()], &job.job_id).is_ok() {
            return Err(io::Error::other(format!("seed need accepted: {seed}")).into());
        }
    }
    if validate_group_needs(&["not a job id!".to_owned()], &job.job_id).is_ok() {
        return Err(io::Error::other("malformed need accepted").into());
    }
    Ok(())
}

#[test]
fn gc1_t08_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_plans()?;
    check_rejects_bad_roots()?;
    check_rejects_seed_race()?;
    if JobTimeout::new(0).is_ok() || JobTimeout::new(JobTimeout::MAX_MINUTES + 1).is_ok() {
        return Err(io::Error::other("out-of-range deadline accepted").into());
    }
    Ok(())
}
