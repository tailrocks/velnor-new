// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, JobId, LaneFeatures};

use super::RootJobs;

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
                let mut steps = self.setup(&id);
                let tests = self
                    .cargo(member, lane.commands)
                    .into_iter()
                    .map(|step| self.heavy(step))
                    .collect();
                steps.extend(self.member_steps(member, tests));
                steps.extend(self.audit());
                steps.extend(self.saves(&id, None, false));
                jobs.push(self.lane_job(id, steps));
            }
        }
        jobs
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
