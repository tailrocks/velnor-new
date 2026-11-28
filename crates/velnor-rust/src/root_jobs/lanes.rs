// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BuildRecord, CargoCommand, Job, JobId, LaneFeatures, RunnerLabel, Step, StepKind, TestNeed,
};

use super::RootJobs;
use crate::plan_tools::mirror_step_on;

const LANE_JOB_PREFIX: &str = "rust-lane";

impl RootJobs<'_> {
    pub(super) fn parallel_lanes(&self) -> Vec<Job> {
        if self.unit.grouped {
            return Vec::new();
        }
        let mut taken: Vec<JobId> = Vec::new();
        let mut jobs = Vec::new();
        for member in &self.members {
            for lane in member.parallel_lanes(self.facts) {
                let id = lane_id(&lane.features, &taken);
                taken.push(id.clone());
                let id = self.id(id);
                let timeout = lane.timeout_minutes;
                let quick = lane.commands.iter().any(CargoCommand::is_compile_check);
                let runner = lane.job.runner.as_ref().map(|runner| &runner.label);
                let mut steps = self.setup(&id);
                if let Some(runner) = runner {
                    steps = self.mirrored_on(steps, runner);
                }
                let mut tests: Vec<Step> = self
                    .cargo(member, lane.commands)
                    .into_iter()
                    .map(|step| self.heavy(step))
                    .collect();
                if lane.job.needs.contains(&TestNeed::Docker) {
                    tests.insert(0, self.docker_check());
                }
                steps.extend(self.member_steps(member, tests));
                steps.extend(self.audit());
                steps.extend(self.saves(&id, None, false));
                let mut job = self.lane_job(id, steps, false);
                if crate::lane::lane_job_fixes(self.facts) {
                    if quick {
                        job.pr = crate::plan::quick(self.facts);
                    }
                    job.build_record = BuildRecord::TestsAndLanes;
                }
                if let Some(minutes) = timeout {
                    job.timeout_minutes = minutes;
                }
                if let Some(runner) = runner {
                    job.runs_on = runner.clone();
                }
                jobs.push(job);
            }
        }
        jobs
    }

    fn mirrored_on(&self, steps: Vec<Step>, runner: &RunnerLabel) -> Vec<Step> {
        steps
            .into_iter()
            .filter_map(|step| match step.kind {
                StepKind::CratesMirror(_) => {
                    mirror_step_on(self.facts, runner).map(|mirror| Step {
                        gate: step.gate,
                        ..mirror
                    })
                }
                _ => Some(step),
            })
            .collect()
    }
}

/// Flags are part of the id, so lanes differing only by a flag get distinct jobs and cache keys.
fn lane_words(features: &LaneFeatures) -> String {
    let mut words = Vec::new();
    if features.all {
        words.push("all-features".to_owned());
    }
    if features.no_default {
        words.push("no-default".to_owned());
    }
    words.extend(features.names.iter().cloned());
    words.join("-")
}

fn lane_id(features: &LaneFeatures, taken: &[JobId]) -> JobId {
    let mut slug = String::new();
    for c in lane_words(features).chars() {
        let c = if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '-'
        };
        if c != '-' || !slug.ends_with('-') {
            slug.push(c);
        }
    }
    let base = format!("{LANE_JOB_PREFIX}-{}", slug.trim_matches('-'));
    let mut name = base.clone();
    let mut copy = 1;
    while taken.iter().any(|id| id.as_str() == name) {
        copy += 1;
        name = format!("{base}-{copy}");
    }
    JobId::new(name).unwrap_or_else(|_| JobId::from_static(LANE_JOB_PREFIX))
}
