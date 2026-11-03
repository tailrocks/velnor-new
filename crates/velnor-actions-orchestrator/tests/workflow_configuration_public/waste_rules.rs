// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::SOURCES_SAVE_CONDITION;
use velnor_actions_contract::workflow::{
    validate_tofu_providers_producer, validate_tools_producer,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::{crate_jobs, mbx_repo, rust_fixtures, tofu_fixture};

const RESTORE_ROLES: [StepRole; 5] = [
    StepRole::ToolsCacheRestore,
    StepRole::CargoSourcesRestore,
    StepRole::MbxBundleRestore,
    StepRole::FamilyRestore,
    StepRole::TofuProvidersRestore,
];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Violation {
    pub(super) rule: &'static str,
    pub(super) job: String,
    pub(super) detail: String,
}

fn violation(rule: &'static str, job: &str, detail: &str) -> Violation {
    Violation {
        rule,
        job: job.to_owned(),
        detail: detail.to_owned(),
    }
}

pub(super) fn w1(jobs: &BTreeMap<String, Job>) -> Vec<Violation> {
    let mut owners: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (id, job) in jobs {
        for step in &job.steps {
            if let StepKind::TaskExecution { task_digest, .. } = &step.kind {
                owners.entry(task_digest).or_default().insert(id);
            }
        }
    }
    owners
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .flat_map(|(digest, ids)| ids.into_iter().map(move |id| violation("w1", id, digest)))
        .collect()
}

fn restore_key(step: &Step) -> Option<&str> {
    if !step.role.is_some_and(|role| RESTORE_ROLES.contains(&role)) {
        return None;
    }
    match &step.kind {
        StepKind::Action { with, .. } => with
            .get("key")
            .or_else(|| with.get("cache-key"))
            .map(String::as_str),
        _ => None,
    }
}

pub(super) fn w4(jobs: &BTreeMap<String, Job>) -> Vec<Violation> {
    let mut found = Vec::new();
    for (id, job) in jobs {
        let mut seen = BTreeSet::new();
        for key in job.steps.iter().filter_map(restore_key) {
            if !seen.insert(key) {
                found.push(violation("w4", id, key));
            }
        }
    }
    found
}

pub(super) fn w5(jobs: &BTreeMap<String, Job>) -> Vec<Violation> {
    jobs.iter()
        .flat_map(|(id, job)| {
            job.steps
                .iter()
                .filter(move |step| match step.role {
                    Some(StepRole::ToolsCacheSave) => validate_tools_producer(job, id).is_err(),
                    Some(StepRole::TofuProvidersSave) => {
                        validate_tofu_providers_producer(job, id).is_err()
                    }
                    _ => false,
                })
                .map(move |step| violation("w5", id, &step.name))
        })
        .collect()
}

fn producer_step(jobs: &BTreeMap<String, Job>, role: StepRole) -> Result<Step, Box<dyn Error>> {
    jobs.iter()
        .filter(|(id, job)| match role {
            StepRole::TofuProvidersSave => validate_tofu_providers_producer(job, id).is_ok(),
            _ => validate_tools_producer(job, id).is_ok(),
        })
        .flat_map(|(_, job)| job.steps.iter())
        .find(|step| step.role == Some(role))
        .cloned()
        .ok_or_else(|| format!("no producer step with role {role:?}").into())
}

fn first_crate_id(jobs: &BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    crate_jobs(jobs)
        .first()
        .map(|(id, _)| (*id).clone())
        .ok_or_else(|| "no crate job".into())
}

fn mutable<'a>(
    jobs: &'a mut BTreeMap<String, Job>,
    id: &str,
) -> Result<&'a mut Job, Box<dyn Error>> {
    jobs.get_mut(id)
        .ok_or_else(|| format!("missing job {id}").into())
}

#[test]
fn w1_no_two_jobs_run_the_same_task() -> Result<(), Box<dyn Error>> {
    let mut fixtures = rust_fixtures()?;
    for fixture in &fixtures {
        assert_eq!(w1(&fixture.jobs), Vec::new(), "{}", fixture.name);
    }
    let mut jobs = fixtures.remove(0).jobs;
    let id = first_crate_id(&jobs)?;
    let clone = mutable(&mut jobs, &id)?.clone();
    jobs.insert(format!("{id}-copy"), clone);
    let flagged: BTreeSet<String> = w1(&jobs).into_iter().map(|found| found.job).collect();
    assert_eq!(flagged, BTreeSet::from([id.clone(), format!("{id}-copy")]));
    assert_eq!(w1(&tofu_fixture()?.jobs), Vec::new());
    Ok(())
}

#[test]
fn w4_no_job_restores_the_same_key_twice() -> Result<(), Box<dyn Error>> {
    let mut fixtures = rust_fixtures()?;
    for fixture in &fixtures {
        assert_eq!(w4(&fixture.jobs), Vec::new(), "{}", fixture.name);
    }
    let mut jobs = fixtures.remove(0).jobs;
    let id = first_crate_id(&jobs)?;
    let job = mutable(&mut jobs, &id)?;
    let restore = job
        .steps
        .iter()
        .find(|step| restore_key(step).is_some())
        .cloned()
        .ok_or("crate job has no keyed restore")?;
    job.steps.push(restore);
    let flagged = w4(&jobs);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].job, id);
    let mut tofu = tofu_fixture()?.jobs;
    assert_eq!(w4(&tofu), Vec::new());
    let tofu_id = tofu
        .iter()
        .find(|(_, job)| {
            job.steps
                .iter()
                .any(|step| step.role == Some(StepRole::TofuProvidersRestore))
        })
        .map(|(id, _)| id.clone())
        .ok_or("no tofu provider restore")?;
    let tofu_job = mutable(&mut tofu, &tofu_id)?;
    let provider_restore = tofu_job
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::TofuProvidersRestore))
        .cloned()
        .ok_or("tofu provider restore vanished")?;
    tofu_job.steps.push(provider_restore);
    let flagged = w4(&tofu);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].job, tofu_id);
    Ok(())
}

#[test]
fn w5_producer_saves_live_only_in_a_producer() -> Result<(), Box<dyn Error>> {
    let mut fixtures = rust_fixtures()?;
    for fixture in &fixtures {
        assert_eq!(w5(&fixture.jobs), Vec::new(), "{}", fixture.name);
        assert!(
            producer_step(&fixture.jobs, StepRole::ToolsCacheSave).is_ok(),
            "{}: no producer save to exempt",
            fixture.name
        );
    }
    let mut jobs = fixtures.remove(0).jobs;
    let save = producer_step(&jobs, StepRole::ToolsCacheSave)?;
    let id = first_crate_id(&jobs)?;
    mutable(&mut jobs, &id)?.steps.push(save);
    let flagged = w5(&jobs);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].job, id);
    let mut tofu = tofu_fixture()?.jobs;
    assert_eq!(w5(&tofu), Vec::new());
    let provider_save = producer_step(&tofu, StepRole::TofuProvidersSave)?;
    let tofu_id = tofu
        .iter()
        .find(|(_, job)| job.producer_kind().is_none() && has_provider_restore(job))
        .map(|(id, _)| id.clone())
        .ok_or("no tofu crate job")?;
    mutable(&mut tofu, &tofu_id)?.steps.push(provider_save);
    let flagged = w5(&tofu);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].job, tofu_id);
    Ok(())
}

fn has_provider_restore(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| step.role == Some(StepRole::TofuProvidersRestore))
}

fn prepared_with_extra_step(
    role_source: StepRole,
    from_producer: bool,
) -> Result<Result<(), velnor_actions_orchestrator::OrchestratorError>, Box<dyn Error>> {
    let fixture = rust_fixtures()?.remove(0);
    let extra = if from_producer {
        producer_step(&fixture.jobs, role_source)?
    } else {
        let id = first_crate_id(&fixture.jobs)?;
        fixture.jobs[&id]
            .steps
            .iter()
            .find(|step| step.role == Some(role_source))
            .cloned()
            .ok_or("crate job has no such step")?
    };
    let repo = mbx_repo(false)?;
    let mut prepared = prepare(repo.path())?;
    let id = prepared
        .workflow
        .ir
        .jobs
        .iter()
        .find(|(id, job)| id.starts_with("rust-") && !job.is_dependency_seed())
        .map(|(id, _)| id.clone())
        .ok_or("no crate job in the prepared workflow")?;
    prepared
        .workflow
        .ir
        .jobs
        .get_mut(&id)
        .ok_or("crate job vanished")?
        .steps
        .push(extra);
    Ok(finalized_jobs(&prepared).map(|_| ()))
}

#[test]
fn render_rejects_a_hand_authored_tools_cache_step() -> Result<(), Box<dyn Error>> {
    for (role, from_producer) in [
        (StepRole::ToolsCacheSave, true),
        (StepRole::ToolsCacheRestore, false),
    ] {
        let message = prepared_with_extra_step(role, from_producer)?
            .err()
            .ok_or_else(|| format!("a hand-authored {role:?} rendered"))?
            .to_string();
        assert!(
            message.contains("renderer_owned_tools_cache_step"),
            "{role:?}: wrong error: {message}"
        );
    }
    Ok(())
}

const SAVE_ROLES: [StepRole; 5] = [
    StepRole::ToolsCacheSave,
    StepRole::CargoSourcesSave,
    StepRole::MbxBundleSave,
    StepRole::FamilySave,
    StepRole::TofuProvidersSave,
];

const TRUST_CONDITION: &str = "needs.plan.outputs.cache_trust == 'trusted'";
const PUSH_CONDITION: &str = "github.event_name == 'push'";

pub(super) fn untrusted_saves(jobs: &BTreeMap<String, Job>) -> Vec<Violation> {
    jobs.iter()
        .filter(|(_, job)| !job.cache_access.is_read())
        .flat_map(|(id, job)| {
            job.steps
                .iter()
                .filter(|step| step.role.is_some_and(|role| SAVE_ROLES.contains(&role)))
                .filter(move |step| {
                    let condition = step.condition.as_deref().unwrap_or_default();
                    let job_condition = job.condition.as_deref().unwrap_or_default();
                    !(condition == SOURCES_SAVE_CONDITION
                        || condition.contains(TRUST_CONDITION)
                        || condition.contains(PUSH_CONDITION)
                        || job_condition.contains(PUSH_CONDITION))
                })
                .map(move |step| violation("trust", id, &step.name))
        })
        .collect()
}

#[test]
fn every_non_read_job_saves_caches_only_for_trusted_runs() -> Result<(), Box<dyn Error>> {
    let mut fixtures = rust_fixtures()?;
    fixtures.push(tofu_fixture()?);
    let mut saved_roles = Vec::new();
    for fixture in &fixtures {
        assert_eq!(
            untrusted_saves(&fixture.jobs),
            Vec::new(),
            "{}",
            fixture.name
        );
        for job in fixture.jobs.values() {
            saved_roles.extend(
                job.steps
                    .iter()
                    .filter_map(|step| step.role)
                    .filter(|role| SAVE_ROLES.contains(role)),
            );
        }
    }
    assert!(
        saved_roles.contains(&StepRole::TofuProvidersSave),
        "{saved_roles:?}"
    );
    assert!(
        saved_roles.contains(&StepRole::ToolsCacheSave),
        "{saved_roles:?}"
    );
    let mut jobs = tofu_fixture()?.jobs;
    let tofu_id = jobs
        .iter()
        .find(|(_, job)| {
            job.steps
                .iter()
                .any(|step| step.role == Some(StepRole::TofuProvidersSave))
        })
        .map(|(id, _)| id.clone())
        .ok_or("no tofu provider save")?;
    let producer = mutable(&mut jobs, &tofu_id)?;
    producer.condition = None;
    for step in &mut producer.steps {
        if step.role == Some(StepRole::TofuProvidersSave) {
            step.condition = Some("success()".to_owned());
        }
    }
    let flagged = untrusted_saves(&jobs);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert_eq!(flagged[0].job, tofu_id);
    Ok(())
}
