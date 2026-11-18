// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{
    SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};
use velnor_actions_orchestrator::{
    WasteFinding, WasteRule, finalized_jobs, prepare, waste_findings,
};

use super::bun_routing::make_bun_repo;
use super::one_job_per_crate::{group_fixtures, tofu_fixture};
use super::waste_lint::step_name;
use super::waste_rules::mutable;

const RECHECK_NAME: &str = "Recheck cache state after queue slot";

pub(super) fn of(jobs: &BTreeMap<String, Job>, rule: WasteRule) -> Vec<WasteFinding> {
    waste_findings(jobs)
        .into_iter()
        .filter(|finding| finding.rule() == rule)
        .collect()
}

pub(super) fn mbx_jobs() -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    Ok(group_fixtures()?.remove(0).jobs)
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

#[test]
fn w4_flags_a_role_less_renamed_copy_of_a_restore() -> Result<(), Box<dyn Error>> {
    let mut all = group_fixtures()?;
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
