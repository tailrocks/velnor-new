// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::SOURCES_SAVE_CONDITION;
use velnor_actions_contract::{Job, StepRole};

use super::one_job_per_crate::{group_fixtures, tofu_fixture};

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

pub(super) fn mutable<'a>(
    jobs: &'a mut BTreeMap<String, Job>,
    id: &str,
) -> Result<&'a mut Job, Box<dyn Error>> {
    jobs.get_mut(id)
        .ok_or_else(|| format!("missing job {id}").into())
}

const SAVE_ROLES: [StepRole; 4] = [
    StepRole::ToolsCacheSave,
    StepRole::CargoSourcesSave,
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
    let mut fixtures = group_fixtures()?;
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
