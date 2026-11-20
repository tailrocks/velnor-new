// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedJob, Rule, RuleViolation};
use crate::workflow::{ShellcheckMarks, yaml_job};
use crate::yaml::emit;

const LARGEST_JOBS: usize = 5;

pub(crate) fn too_large(path: &str, bytes: usize, jobs: &[RenderedJob]) -> RuleViolation {
    let mut sizes: Vec<(usize, &str)> = jobs
        .iter()
        .map(|job| {
            let rendered = emit(&[(job.id.clone(), yaml_job(job, ShellcheckMarks::Jobs))]);
            (rendered.len(), job.id.as_str())
        })
        .collect();
    sizes.sort_by(|left, right| right.cmp(left));
    let largest: Vec<String> = sizes
        .iter()
        .take(LARGEST_JOBS)
        .map(|(size, id)| format!("{id} {size} bytes"))
        .collect();
    RuleViolation {
        job: format!(
            "{path} has {} jobs; the largest are {}",
            jobs.len(),
            largest.join(", ")
        ),
        step: 0,
        rule: Rule::WorkflowTooLarge(bytes),
    }
}
