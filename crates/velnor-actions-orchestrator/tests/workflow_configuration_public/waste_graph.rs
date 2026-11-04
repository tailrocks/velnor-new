// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{
    SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR,
};
use velnor_actions_contract::{HOSTED_SUFFIX, Job, SCALE_SUFFIX, Step, StepKind, StepRole};
use velnor_actions_orchestrator::{
    WasteFinding, WasteRule, finalized_jobs, prepare, waste_findings,
};

use super::bun_routing::make_bun_repo;
use super::one_job_per_crate::{rust_fixtures, tofu_fixture};
use super::waste_lint::step_name;
use super::waste_rules::{first_crate_id, mutable, producer_step};

const RECHECK_NAME: &str = "Recheck cache state after queue slot";

pub(super) fn of(jobs: &BTreeMap<String, Job>, rule: WasteRule) -> Vec<WasteFinding> {
    waste_findings(jobs)
        .into_iter()
        .filter(|finding| finding.rule() == rule)
        .collect()
}

pub(super) fn digests(job: &Job) -> BTreeSet<String> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution { task_digest, .. } => Some(task_digest.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn mbx_jobs() -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    Ok(rust_fixtures()?.remove(0).jobs)
}

fn restore_key(step: &Step) -> Option<&str> {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return None;
    };
    if !uses.starts_with("actions/cache/restore@")
        && step.role != Some(StepRole::TofuProvidersRestore)
    {
        return None;
    }
    with.get("key")
        .or_else(|| with.get("cache-key"))
        .map(String::as_str)
}

pub(super) fn recheck_copy(step: &Step) -> Step {
    let mut copy = step.clone();
    copy.id = None;
    RECHECK_NAME.clone_into(&mut copy.name);
    copy.condition = None;
    copy.role = None;
    copy
}

pub(super) fn repeat_restore(
    jobs: &mut BTreeMap<String, Job>,
    pick: impl Fn(&Step) -> bool,
    rekey: Option<&str>,
) -> Result<(String, String), Box<dyn Error>> {
    let (id, restore) = jobs
        .iter()
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .find(|step| restore_key(step).is_some() && pick(step))
                .map(|step| (id.clone(), step.clone()))
        })
        .ok_or("no matching keyed restore")?;
    let mut copy = recheck_copy(&restore);
    if let (Some(key), StepKind::Action { with, .. }) = (rekey, &mut copy.kind) {
        with.insert("key".to_owned(), key.to_owned());
    }
    mutable(jobs, &id)?.steps.push(copy);
    let key = restore_key(&restore)
        .ok_or("restore lost its key")?
        .to_owned();
    Ok((id, key))
}

pub(super) fn clone_crate_job(jobs: &mut BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    let id = first_crate_id(jobs)?;
    let clone = mutable(jobs, &id)?.clone();
    jobs.insert(format!("{id}-copy"), clone);
    Ok(id)
}

#[test]
fn w1_flags_every_task_digest_two_jobs_run() -> Result<(), Box<dyn Error>> {
    let mut all = rust_fixtures()?;
    all.push(tofu_fixture()?);
    for fixture in &all {
        assert_eq!(
            of(&fixture.jobs, WasteRule::DuplicateTask),
            Vec::new(),
            "{}",
            fixture.name
        );
    }
    let mut jobs = mbx_jobs()?;
    let id = clone_crate_job(&mut jobs)?;
    let task_digests = digests(&jobs[&id]);
    assert!(!task_digests.is_empty(), "crate job runs no task");
    let mut expected: Vec<WasteFinding> = task_digests
        .into_iter()
        .map(|digest| WasteFinding::DuplicateTask {
            digest,
            jobs: vec![id.clone(), format!("{id}-copy")],
        })
        .collect();
    expected.sort();
    assert_eq!(of(&jobs, WasteRule::DuplicateTask), expected);
    Ok(())
}

#[test]
fn w1_treats_the_hosted_and_local_lane_twins_as_one_job() -> Result<(), Box<dyn Error>> {
    let mut jobs = mbx_jobs()?;
    let id = first_crate_id(&jobs)?;
    let job = jobs.remove(&id).ok_or("crate job vanished")?;
    jobs.insert(format!("{id}{HOSTED_SUFFIX}"), job.clone());
    jobs.insert(format!("{id}{SCALE_SUFFIX}"), job.clone());
    assert_eq!(of(&jobs, WasteRule::DuplicateTask), Vec::new());
    jobs.insert(format!("{id}-copy{HOSTED_SUFFIX}"), job.clone());
    let task_digests = digests(&job);
    assert!(!task_digests.is_empty(), "crate job runs no task");
    let mut expected: Vec<WasteFinding> = task_digests
        .into_iter()
        .map(|digest| WasteFinding::DuplicateTask {
            digest,
            jobs: vec![id.clone(), format!("{id}-copy")],
        })
        .collect();
    expected.sort();
    assert_eq!(of(&jobs, WasteRule::DuplicateTask), expected);
    Ok(())
}

#[test]
fn w1_lets_one_producer_repeat_a_crate_digest_but_not_two() -> Result<(), Box<dyn Error>> {
    let mut jobs = mbx_jobs()?;
    let crate_id = first_crate_id(&jobs)?;
    let task = jobs[&crate_id]
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .cloned()
        .ok_or("crate job runs no task")?;
    let StepKind::TaskExecution { task_digest, .. } = &task.kind else {
        return Err("not a task step".into());
    };
    let producer_id = jobs
        .iter()
        .find(|(_, job)| job.producer_kind().is_some())
        .map(|(id, _)| id.clone())
        .ok_or("no producer job")?;
    mutable(&mut jobs, &producer_id)?.steps.push(task.clone());
    assert_eq!(
        of(&jobs, WasteRule::DuplicateTask),
        Vec::new(),
        "a producer sharing a crate digest was flagged"
    );
    let copy = jobs[&producer_id].clone();
    jobs.insert(format!("{producer_id}-copy"), copy);
    assert_eq!(
        of(&jobs, WasteRule::DuplicateTask),
        vec![WasteFinding::DuplicateTask {
            digest: task_digest.clone(),
            jobs: vec![producer_id.clone(), format!("{producer_id}-copy")],
        }]
    );
    Ok(())
}

#[test]
fn w4_flags_a_role_less_renamed_copy_of_a_restore() -> Result<(), Box<dyn Error>> {
    let mut all = rust_fixtures()?;
    all.push(tofu_fixture()?);
    for fixture in &all {
        assert_eq!(
            of(&fixture.jobs, WasteRule::DuplicateRestore),
            Vec::new(),
            "{}",
            fixture.name
        );
    }
    let mut jobs = mbx_jobs()?;
    let (job, key) = repeat_restore(
        &mut jobs,
        |step| step.role == Some(StepRole::ToolsCacheRestore),
        None,
    )?;
    assert_eq!(
        of(&jobs, WasteRule::DuplicateRestore),
        vec![WasteFinding::DuplicateRestore { job, key }]
    );
    let mut tofu = tofu_fixture()?.jobs;
    let (job, key) = repeat_restore(
        &mut tofu,
        |step| step.role == Some(StepRole::TofuProvidersRestore),
        None,
    )?;
    assert_eq!(
        of(&tofu, WasteRule::DuplicateRestore),
        vec![WasteFinding::DuplicateRestore { job, key }]
    );
    Ok(())
}

#[test]
fn w4_treats_both_sources_key_expressions_as_one_key() -> Result<(), Box<dyn Error>> {
    let mut jobs = mbx_jobs()?;
    let (job, _) = repeat_restore(
        &mut jobs,
        |step| {
            matches!(&step.kind, StepKind::Action { with, .. }
                if with.get("key").is_some_and(|key| key == SOURCES_KEY_PLAN_EXPR || key == SOURCES_KEY_STEP_EXPR))
        },
        Some(SOURCES_KEY_STEP_EXPR),
    )?;
    assert_eq!(
        of(&jobs, WasteRule::DuplicateRestore),
        vec![WasteFinding::DuplicateRestore {
            job,
            key: SOURCES_KEY_PLAN_EXPR.to_owned(),
        }]
    );
    Ok(())
}

pub(super) fn save_in_crate_job(
    jobs: &mut BTreeMap<String, Job>,
) -> Result<(String, String), Box<dyn Error>> {
    let save = producer_step(jobs, StepRole::ToolsCacheSave)?;
    let id = first_crate_id(jobs)?;
    let name = save.name.clone();
    mutable(jobs, &id)?.steps.push(save);
    Ok((id, name))
}

#[test]
fn w5_flags_a_cache_save_outside_a_producer_job() -> Result<(), Box<dyn Error>> {
    let mut all = rust_fixtures()?;
    all.push(tofu_fixture()?);
    for fixture in &all {
        assert_eq!(
            of(&fixture.jobs, WasteRule::NonProducerSave),
            Vec::new(),
            "{}",
            fixture.name
        );
        assert!(
            producer_step(&fixture.jobs, StepRole::ToolsCacheSave).is_ok(),
            "{}: no producer save to exempt",
            fixture.name
        );
    }
    let mut jobs = mbx_jobs()?;
    let (job, step) = save_in_crate_job(&mut jobs)?;
    assert_eq!(
        of(&jobs, WasteRule::NonProducerSave),
        vec![WasteFinding::NonProducerSave { job, step }]
    );
    Ok(())
}

#[test]
fn w5_allows_a_stack_family_save_only_while_it_renders_false() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_repo()?;
    velnor_actions_orchestrator::migrate_config(repo.path(), 2, true)?;
    let mut jobs = finalized_jobs(&prepare(repo.path())?)?;
    let (job, at) = jobs
        .iter()
        .filter(|(_, job)| job.producer_kind().is_none())
        .find_map(|(id, job)| {
            job.steps
                .iter()
                .position(|step| step.role == Some(StepRole::FamilySave))
                .map(|at| (id.clone(), at))
        })
        .ok_or("no family save in a stack job")?;
    assert_eq!(jobs[&job].steps[at].condition.as_deref(), Some("false"));
    assert_eq!(of(&jobs, WasteRule::NonProducerSave), Vec::new());
    let step = step_name(&jobs, &job, at);
    mutable(&mut jobs, &job)?.steps[at].condition = Some("success()".to_owned());
    assert_eq!(
        of(&jobs, WasteRule::NonProducerSave),
        vec![WasteFinding::NonProducerSave { job, step }]
    );
    Ok(())
}
