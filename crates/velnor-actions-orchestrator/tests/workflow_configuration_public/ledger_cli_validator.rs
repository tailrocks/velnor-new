// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_JOB_ID, LedgerCliSource, validate_ledger_cli_jobs,
};
use velnor_actions_contract::{
    Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, Step, StepKind, StepRole, WorkflowPolicy,
};

use velnor_actions_workflow_renderer::plan_sources::source_cache::plan_sources_steps;

use super::ledger_cli_job::velnor_fixtures;
use super::one_job_per_crate::{Rendered, crate_jobs, rust_fixtures};

type Jobs = BTreeMap<String, Job>;

fn first_crate(jobs: &Jobs) -> Result<String, Box<dyn Error>> {
    crate_jobs(jobs)
        .first()
        .map(|(id, _)| (*id).clone())
        .ok_or_else(|| "no crate job".into())
}

fn step_at(job: &Job, role: StepRole) -> Result<usize, Box<dyn Error>> {
    job.steps
        .iter()
        .position(|step| step.role == Some(role))
        .ok_or_else(|| format!("no {role:?} step").into())
}

fn emit_at(job: &Job) -> Result<usize, Box<dyn Error>> {
    job.steps
        .iter()
        .position(|step| step.name == "Emit group ledger")
        .ok_or_else(|| "no emit step".into())
}

type Mutation = fn(&mut Jobs, &str) -> Result<(), Box<dyn Error>>;

fn mutations() -> Vec<(&'static str, Mutation)> {
    let mut all: Vec<(&'static str, Mutation)> = vec![
        ("second build job", |jobs, _| {
            let clone = jobs[LEDGER_CLI_JOB_ID].clone();
            jobs.insert("ledger-cli-second".to_owned(), clone);
            Ok(())
        }),
        ("missing verify", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliVerify)?;
            job.steps.remove(at);
            Ok(())
        }),
        ("verify after emit", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliVerify)?;
            let verify = job.steps.remove(at);
            let emit = emit_at(job)?;
            job.steps.insert(emit + 1, verify);
            Ok(())
        }),
        ("extra permissions", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            job.permissions = Some(Permissions {
                packages: PermissionLevel::Write,
                ..Permissions::default()
            });
            Ok(())
        }),
        ("secret in build", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliBuild)?;
            let StepKind::Shell { env, .. } = &mut job.steps[at].kind else {
                return Err("build is not a shell step".into());
            };
            env.insert("LEAK".to_owned(), "${{ secrets.TOKEN }}".to_owned());
            Ok(())
        }),
        ("literal artifact name", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliDownload)?;
            let StepKind::Action { with, .. } = &mut job.steps[at].kind else {
                return Err("download is not an action step".into());
            };
            with.insert("name".to_owned(), "velnor-ledger-cli".to_owned());
            Ok(())
        }),
        ("conditional download", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliDownload)?;
            job.steps[at].condition = None;
            Ok(())
        }),
        ("plain emit condition", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let at = emit_at(job)?;
            job.steps[at].condition = Some("always()".to_owned());
            Ok(())
        }),
        ("needs without ledger-cli", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            job.needs.retain(|need| need != LEDGER_CLI_JOB_ID);
            Ok(())
        }),
        ("required waits on ledger-cli", |jobs, _| {
            let required = jobs.get_mut(REQUIRED_JOB_ID).ok_or("required")?;
            required.needs.push(LEDGER_CLI_JOB_ID.to_owned());
            Ok(())
        }),
    ];
    all.extend(sources_mutations());
    all.extend(artifact_mutations());
    all
}

fn cloned_step(job: &Job, role: StepRole) -> Result<Step, Box<dyn Error>> {
    Ok(job.steps[step_at(job, role)?].clone())
}

fn untagged_download(job: &Job) -> Result<Step, Box<dyn Error>> {
    let mut step = cloned_step(job, StepRole::LedgerCliDownload)?;
    "Download extra".clone_into(&mut step.name);
    step.role = None;
    let StepKind::Action { with, .. } = &mut step.kind else {
        return Err("download is not an action step".into());
    };
    with.insert("name".to_owned(), "velnor-extra".to_owned());
    with.insert(
        "path".to_owned(),
        "${{ runner.temp }}/velnor/bins".to_owned(),
    );
    Ok(step)
}

fn run_downloaded() -> Step {
    Step {
        parallel: false,
        name: "Run downloaded".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["$RUNNER_TEMP/velnor/bins/x".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn artifact_mutations() -> Vec<(&'static str, Mutation)> {
    vec![
        ("one day retention", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliUpload)?;
            let StepKind::Action { with, .. } = &mut job.steps[at].kind else {
                return Err("upload is not an action step".into());
            };
            with.insert("retention-days".to_owned(), "1".to_owned());
            Ok(())
        }),
        ("extra artifact download in crate job", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let extra = untagged_download(job)?;
            let emit = emit_at(job)?;
            job.steps.insert(emit, extra);
            Ok(())
        }),
        ("downloaded artifact executed in crate job", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let mut extra = untagged_download(job)?;
            extra.role = Some(StepRole::DownloadPlan);
            let emit = emit_at(job)?;
            job.steps.insert(emit, run_downloaded());
            job.steps.insert(emit, extra);
            Ok(())
        }),
        ("crate job uploads compiled output", |jobs, id| {
            let job = jobs.get_mut(id).ok_or("job")?;
            let mut extra = cloned_step(job, StepRole::MatrixReportUpload)?;
            extra.role = None;
            let StepKind::Action { with, .. } = &mut extra.kind else {
                return Err("upload is not an action step".into());
            };
            with.insert("name".to_owned(), "velnor-compiled".to_owned());
            with.insert(
                "path".to_owned(),
                "${{ env.CARGO_TARGET_DIR }}/debug".to_owned(),
            );
            let emit = emit_at(job)?;
            job.steps.insert(emit, extra);
            Ok(())
        }),
    ]
}

fn sources_mutations() -> Vec<(&'static str, Mutation)> {
    vec![
        ("missing sources restore", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::CargoSourcesRestore)?;
            job.steps.remove(at);
            Ok(())
        }),
        ("sources restore after fetch", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::CargoSourcesRestore)?;
            let restore = job.steps.remove(at);
            let fetch = step_at(job, StepRole::CargoSourcesFetch)?;
            job.steps.insert(fetch + 1, restore);
            Ok(())
        }),
        ("sources save", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let (_, save) = plan_sources_steps().map_err(|err| format!("{err:?}"))?;
            let build = step_at(job, StepRole::LedgerCliBuild)?;
            job.steps.insert(build + 1, save);
            Ok(())
        }),
        ("online build", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliBuild)?;
            let StepKind::Shell { run, .. } = &mut job.steps[at].kind else {
                return Err("build is not a shell step".into());
            };
            run.retain(|arg| arg != "--offline");
            Ok(())
        }),
        ("rustup auto install", |jobs, _| {
            let job = jobs.get_mut(LEDGER_CLI_JOB_ID).ok_or("job")?;
            let at = step_at(job, StepRole::LedgerCliBuild)?;
            let StepKind::Shell { env, .. } = &mut job.steps[at].kind else {
                return Err("build is not a shell step".into());
            };
            env.remove("RUSTUP_AUTO_INSTALL");
            Ok(())
        }),
    ]
}

#[test]
fn ledger_cli_validator_rejects_each_mutation() -> Result<(), Box<dyn Error>> {
    for Rendered { name, jobs, .. } in velnor_fixtures()? {
        validate_ledger_cli_jobs(&jobs, LedgerCliSource::SameRunBuild)?;
        let id = first_crate(&jobs)?;
        let mut survivors = Vec::new();
        for (mutation, apply) in mutations() {
            let mut mutated = jobs.clone();
            apply(&mut mutated, &id)?;
            if validate_ledger_cli_jobs(&mutated, LedgerCliSource::SameRunBuild).is_ok() {
                survivors.push(mutation);
            }
        }
        assert_eq!(survivors, Vec::<&str>::new(), "{name}: mutations survived");
    }
    Ok(())
}

#[test]
fn consumer_policy_rejects_cross_job_artifacts() -> Result<(), Box<dyn Error>> {
    let fixture = rust_fixtures()?
        .into_iter()
        .find(|fixture| fixture.policy == WorkflowPolicy::ConsumerV1)
        .ok_or("no consumer fixture")?;
    validate_ledger_cli_jobs(&fixture.jobs, LedgerCliSource::Installed)?;
    let id = first_crate(&fixture.jobs)?;
    let mut jobs = fixture.jobs.clone();
    let job = jobs.get_mut(&id).ok_or("job")?;
    let mut extra = cloned_step(job, StepRole::DownloadPlan)?;
    extra.role = None;
    let StepKind::Action { with, .. } = &mut extra.kind else {
        return Err("download is not an action step".into());
    };
    with.insert("name".to_owned(), "velnor-ledger-cli".to_owned());
    let emit = emit_at(job)?;
    job.steps.insert(emit, extra);
    assert!(validate_ledger_cli_jobs(&jobs, LedgerCliSource::Installed).is_err());
    Ok(())
}
