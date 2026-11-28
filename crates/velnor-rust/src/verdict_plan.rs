// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    GateNeed, JobCondition, JobId, MERGE_GROUP_SKIP_OUTPUT, REUSE_OUTPUT, REUSE_RUN_OUTPUT,
    REUSED_OUTPUT, RootSlug, VerdictSpec, group_output_name,
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
    pub reused_lanes: BTreeSet<(JobId, RootSlug)>,
    pub reused_jobs: BTreeSet<JobId>,
    pub reuse_notes: Vec<String>,
}

fn reused_ids<'a>(outputs: &'a BTreeMap<String, String>, event: Option<&str>) -> BTreeSet<&'a str> {
    let verified = event == Some("merge_group")
        && outputs.get(REUSE_OUTPUT).map(String::as_str) == Some("true");
    outputs
        .get(REUSED_OUTPUT)
        .filter(|_| verified)
        .into_iter()
        .flat_map(|ids| ids.split_whitespace())
        .collect()
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
        let merge_group_ran = event == Some(PUSH)
            && outputs.get(MERGE_GROUP_SKIP_OUTPUT).map(String::as_str) == Some("true");
        let reused = reused_ids(outputs, event);
        let mut reused_jobs = BTreeSet::new();
        let mut planned = BTreeMap::new();
        let mut test_lanes: BTreeMap<RootSlug, Vec<JobId>> = BTreeMap::new();
        let mut reused_lanes = BTreeSet::new();
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
                    JobCondition::Lint(slug) => {
                        flag(GateNeed::Lint.output_name(slug), "run", "skip")?
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
                    JobCondition::MergeGroupNotPassed => !merge_group_ran,
                };
            }
            planned.insert(job.id.clone(), holds);
            if job.reusable && reused.contains(job.id.as_str()) {
                reused_jobs.insert(job.id.clone());
            }
            for slug in &job.tests {
                let expected = flag(GateNeed::TestsExpected.output_name(slug), "run", "skip")?;
                let lane_reused = job
                    .reusable_tests
                    .get(slug)
                    .is_some_and(|ids| ids.iter().all(|id| reused.contains(id.as_str())));
                let jobs = test_lanes.entry(slug.clone()).or_default();
                if expected {
                    jobs.push(job.id.clone());
                    if lane_reused {
                        reused_lanes.insert((job.id.clone(), slug.clone()));
                    }
                }
            }
        }
        test_lanes.retain(|_, jobs| !merge_group_ran && !jobs.is_empty());
        let run = outputs.get(REUSE_RUN_OUTPUT).map_or("", String::as_str);
        Ok(Self {
            planned,
            test_lanes,
            reused_lanes,
            reused_jobs,
            reuse_notes: reused
                .iter()
                .map(|id| format!("reused: {id} passed in {run}"))
                .collect(),
        })
    }
}

fn event_is(event: Option<&str>, names: &[&str]) -> Result<bool, String> {
    event
        .filter(|name| !name.is_empty())
        .map(|name| names.contains(&name))
        .ok_or_else(|| "GITHUB_EVENT_NAME is not set".to_owned())
}

#[cfg(test)]
mod tests {
    use velnor_model::{JobCondition, VerdictJob};

    use super::*;

    fn planned(skip: &str, event: &str) -> bool {
        let job = JobId::from_static("rust-test-default");
        let spec = VerdictSpec {
            plan: JobId::from_static("rust-plan"),
            jobs: vec![VerdictJob::new(
                job.clone(),
                vec![JobCondition::MergeGroupNotPassed],
                Vec::new(),
            )],
        };
        let outputs = BTreeMap::from([(MERGE_GROUP_SKIP_OUTPUT.to_owned(), skip.to_owned())]);
        PlanOutput::resolve(&spec, &outputs, Some(event))
            .expect("the spec resolves")
            .planned[&job]
    }

    #[test]
    fn a_job_skipped_after_a_passed_queue_run_is_not_planned() {
        assert!(!planned("true", "push"));
        assert!(planned("false", "push"));
        assert!(planned("true", "pull_request"));
    }
}
