// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{MAX_WORKFLOW_BYTES, Rule};

#[path = "size_report.rs"]
pub(crate) mod size_report;

pub(crate) fn whole_workflow(rule: &Rule, job: &str) -> Option<String> {
    match rule {
        Rule::WorkflowTooLarge(bytes) => {
            let detail = if job.is_empty() {
                String::new()
            } else {
                format!(" ({job})")
            };
            Some(format!(
                "the workflow is {bytes} bytes, and GitHub does not start a workflow file above {MAX_WORKFLOW_BYTES} bytes{detail}; reduce the workflow size"
            ))
        }
        Rule::OutputNameCollision(name) => Some(format!(
            "the tag job would define the output {name} twice: it is both a released package and the commit output of another package; rename the package"
        )),
        Rule::SourceRootsEnvCollision(name) => Some(format!(
            "two different Cargo root lists would both be written to the workflow env entry {name}; the name is a hash of the list, so this is a hash collision and the generator will not let one list overwrite the other"
        )),
        Rule::DistFileCollision(name) => Some(format!(
            "job {job} downloads two artifacts that both contain {name} into the same dist folder, so one would overwrite the other; give the binaries distinct asset names"
        )),
        _ => None,
    }
}
