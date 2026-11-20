// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{WorkflowPlan, seal};

use crate::events::{concurrency, triggers};
use crate::permission_yaml::permissions;
use crate::rules::RenderedWorkflow;
use crate::workflow::{ShellcheckMarks, yaml_job};
use crate::yaml::{Yaml, emit, entry, text};
use crate::{GENERATED_HEADER, env, helper_integrity};

pub(crate) fn document(
    plan: &WorkflowPlan,
    rendered: &RenderedWorkflow,
    roots: &BTreeMap<String, String>,
    marks: ShellcheckMarks,
) -> String {
    let workflow: Vec<_> = [
        Some(entry("name", text(&plan.name))),
        Some(entry("on", triggers(&plan.triggers))),
        Some(entry("permissions", permissions(&plan.permissions))),
        Some(entry("concurrency", concurrency(plan.concurrency))),
        Some(entry("env", env::workflow_env(roots))),
        helper_integrity::defaults(&rendered.jobs).map(|defaults| entry("defaults", defaults)),
        Some(entry(
            "jobs",
            Yaml::Map(
                rendered
                    .jobs
                    .iter()
                    .map(|job| (job.id.clone(), yaml_job(job, marks)))
                    .collect(),
            ),
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    seal(GENERATED_HEADER, &emit(&workflow))
}
