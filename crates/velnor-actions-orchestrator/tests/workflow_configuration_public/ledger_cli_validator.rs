// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_JOB_ID, LedgerCliSource, validate_ledger_cli_jobs,
};
use velnor_actions_contract::{
    Job, PermissionLevel, Permissions, REQUIRED_JOB_ID, StepKind, StepRole,
};

use super::ledger_cli_job::velnor_fixtures;
use super::one_job_per_crate::{Rendered, crate_jobs};

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
    vec![
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
