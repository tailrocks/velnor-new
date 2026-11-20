// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{Rule, RuleViolation};

const LARGEST_JOBS: usize = 5;
const JOBS_KEY: &str = "\njobs:\n";

pub(crate) fn too_large(path: &str, file: &str) -> RuleViolation {
    let mut sizes = job_sizes(file);
    let count = sizes.len();
    sizes.sort_by(|left, right| right.cmp(left));
    let largest: Vec<String> = sizes
        .iter()
        .take(LARGEST_JOBS)
        .map(|(size, id)| format!("{id} {size} bytes"))
        .collect();
    RuleViolation {
        job: format!(
            "{path} has {count} jobs; the largest are {}",
            largest.join(", ")
        ),
        step: 0,
        rule: Rule::WorkflowTooLarge(file.len()),
    }
}

fn job_sizes(file: &str) -> Vec<(usize, &str)> {
    let Some(at) = file.find(JOBS_KEY) else {
        return Vec::new();
    };
    let mut sizes: Vec<(usize, &str)> = Vec::new();
    let mut pending = 0;
    for line in file[at + JOBS_KEY.len()..].split_inclusive('\n') {
        let header = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(":\n"));
        let job_comment = line.starts_with("  #");
        match (header, sizes.last_mut()) {
            (Some(id), _) => {
                sizes.push((pending + line.len(), id));
                pending = 0;
            }
            (None, _) if job_comment => pending += line.len(),
            (None, Some((size, _))) => *size += line.len(),
            (None, None) => {}
        }
    }
    sizes
}
