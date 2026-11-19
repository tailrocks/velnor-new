// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    GateNeed, JobCondition, JobId, MERGE_GROUP_SKIP_OUTPUT, RootSlug, VerdictSpec,
    group_output_name,
};

const SCHEDULE: &str = "schedule";
const PUSH: &str = "push";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Need {
    pub result: Option<String>,
    pub outputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutput {
    pub planned: BTreeMap<JobId, bool>,
    pub test_lanes: BTreeMap<RootSlug, Vec<JobId>>,
    pub advisory: BTreeSet<JobId>,
}

impl PlanOutput {
    pub fn resolve(
        spec: &VerdictSpec,
        outputs: &BTreeMap<String, String>,
        event: Option<&str>,
    ) -> Result<Self, String> {
        let read = |key: String| -> Result<&str, String> {
            outputs
                .get(&key)
                .map(String::as_str)
                .ok_or_else(|| format!("the plan output {key} is missing"))
        };
        let flag = |key: String, on: &str, off: &str| -> Result<bool, String> {
            let value = read(key.clone())?;
            if value == on {
                Ok(true)
            } else if value == off {
                Ok(false)
            } else {
                Err(format!(
                    "the plan output {key} is `{value}`, not {on} or {off}"
                ))
            }
        };
        let mut planned = BTreeMap::new();
        let mut test_lanes: BTreeMap<RootSlug, Vec<JobId>> = BTreeMap::new();
        let mut advisory = BTreeSet::new();
        for job in &spec.jobs {
            let mut holds = true;
            for condition in &job.when {
                holds &= match condition {
                    JobCondition::Lane(slug) => {
                        let key = GateNeed::Selected.output_name(slug);
                        match read(key.clone())? {
                            "none" => false,
                            "some" | "all" => true,
                            other => {
                                return Err(format!(
                                    "the plan output {key} is `{other}`, not none, some or all"
                                ));
                            }
                        }
                    }
                    JobCondition::Group(slug, name) => {
                        flag(group_output_name(slug, name), "run", "skip")?
                    }
                    JobCondition::Policy => {
                        let slug = RootSlug::from_static("policy");
                        let deny = flag(GateNeed::Deny.output_name(&slug), "run", "skip")?;
                        let machete = flag(GateNeed::Machete.output_name(&slug), "run", "skip")?;
                        deny || machete
                    }
                    JobCondition::Schedule => event_is(event, &[SCHEDULE])?,
                    JobCondition::ScheduleOrDispatch => {
                        event_is(event, &[SCHEDULE, "workflow_dispatch"])?
                    }
                    JobCondition::ScheduleOrPushOrDispatch => {
                        event_is(event, &[SCHEDULE, PUSH, "workflow_dispatch"])?
                    }
                    JobCondition::ScheduleOrPush => event_is(event, &[SCHEDULE, PUSH])?,
                    JobCondition::DefaultBranchPush => event_is(event, &[PUSH])?,
                    JobCondition::Advisory => true,
                };
            }
            if job.when.contains(&JobCondition::Advisory) {
                advisory.insert(job.id.clone());
            }
            planned.insert(job.id.clone(), holds);
            for slug in &job.tests {
                let expected = flag(GateNeed::TestsExpected.output_name(slug), "run", "skip")?;
                let jobs = test_lanes.entry(slug.clone()).or_default();
                if expected {
                    jobs.push(job.id.clone());
                }
            }
        }
        let merge_group_ran = event == Some(PUSH)
            && outputs.get(MERGE_GROUP_SKIP_OUTPUT).map(String::as_str) == Some("true");
        test_lanes.retain(|_, jobs| !merge_group_ran && !jobs.is_empty());
        Ok(Self {
            planned,
            test_lanes,
            advisory,
        })
    }
}

fn event_is(event: Option<&str>, names: &[&str]) -> Result<bool, String> {
    event
        .filter(|name| !name.is_empty())
        .map(|name| names.contains(&name))
        .ok_or_else(|| "GITHUB_EVENT_NAME is not set".to_owned())
}
