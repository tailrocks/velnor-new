// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::RunnerSpec;
use super::features::{finish, gated, lane_base, run_step};
use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

pub(super) const MODE_GUARD_ID: &str = "mode-guard";

const STATUS_FUNCTIONS: [&str; 5] = [
    "needs.",
    "always()",
    "success()",
    "failure()",
    "cancelled()",
];

fn condition(job: &Yaml) -> Option<&str> {
    let Yaml::Map(fields) = job else {
        return None;
    };
    fields.iter().find_map(|(key, value)| match value {
        Yaml::Str(text) if key == "if" => Some(text.as_str()),
        _ => None,
    })
}

pub(super) fn job(
    jobs: &[(String, Yaml)],
    hosted: &RunnerSpec,
) -> Result<Option<(String, Yaml)>, RenderError> {
    let mut conditions: Vec<&str> = Vec::new();
    for (id, job) in jobs {
        let Some(when) = condition(job) else {
            return Ok(None);
        };
        if STATUS_FUNCTIONS.iter().any(|token| when.contains(token)) {
            return Err(RenderError::InvalidWorkflow(format!(
                "qualification_job_condition_not_mode_only:{id}"
            )));
        }
        if !conditions.contains(&when) {
            conditions.push(when);
        }
    }
    let selected = conditions
        .iter()
        .map(|when| format!("({when})"))
        .collect::<Vec<_>>()
        .join(" || ");
    Ok(Some(gated(
        finish(
            MODE_GUARD_ID,
            lane_base("Qualification / mode guard", hosted, JobTimeout::DEFAULT),
            vec![run_step(
                "Reject a dispatch that selects no qualification job",
                "echo '::error title=qualification_dispatch_selects_no_job::This dispatch selects no qualification job: the mode is not one of the declared modes, or stable-sources was dispatched from the default branch or a non-branch ref. Every job would be skipped and a skipped run reports success.' && exit 1",
            )],
        ),
        &format!("!({selected})"),
    )))
}
