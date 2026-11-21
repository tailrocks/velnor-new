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
        if line.starts_with(|c: char| !matches!(c, ' ' | '\n' | '\r' | '#')) {
            break;
        }
        let header = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(":\n"));
        let job_comment = line.starts_with("  #");
        match header {
            Some(id) => {
                sizes.push((pending + line.len(), id));
                pending = 0;
            }
            None if job_comment
                || sizes.is_empty()
                || (pending > 0 && !line.starts_with("   ")) =>
            {
                pending += line.len();
            }
            None => {
                if let Some((size, _)) = sizes.last_mut() {
                    *size += pending + line.len();
                }
                pending = 0;
            }
        }
    }
    if let Some((size, _)) = sizes.last_mut() {
        *size += pending;
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::{JOBS_KEY, job_sizes};

    fn header_len(file: &str) -> usize {
        file.find(JOBS_KEY).expect("jobs key") + JOBS_KEY.len()
    }

    #[test]
    fn every_byte_of_the_jobs_section_is_charged_to_a_job() {
        let file = "on: push\njobs:\n\n  # first\n  a:\n    runs-on: x\n\n  # second\n  b:\n    runs-on: y\n  # end\n# trailer\n";
        let total: usize = job_sizes(file).iter().map(|(size, _)| size).sum();
        assert_eq!(total, file.len() - header_len(file));
    }

    #[test]
    fn a_deeper_line_after_a_job_comment_stays_with_the_previous_job() {
        let file = "on: push\njobs:\n  a:\n    runs-on: x\n  # note\n    steps:\n      - run: x\n  b:\n    runs-on: y\n";
        let sizes = job_sizes(file);
        assert_eq!(
            sizes,
            vec![
                (
                    "  a:\n    runs-on: x\n  # note\n    steps:\n      - run: x\n".len(),
                    "a"
                ),
                ("  b:\n    runs-on: y\n".len(), "b"),
            ]
        );
    }

    #[test]
    fn a_top_level_key_after_the_jobs_block_is_not_charged_to_the_last_job() {
        let file = "on: push\njobs:\n  a:\n    runs-on: x\nconcurrency:\n  group: g\n";
        let sizes = job_sizes(file);
        assert_eq!(sizes, vec![("  a:\n    runs-on: x\n".len(), "a")]);
    }
}
