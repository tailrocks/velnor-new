// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BUDGET_JOB_NAME, GateNeed, Job, Selection, SelectionName};

use crate::gate::check_name;

/// The expression for the title of a job, when it needs one: the title of the
/// job that records the selection of its run, or the title of a job whose check
/// the pull request events may ignore.
pub(crate) fn title(job: &Job) -> Option<String> {
    match &job.selection_name {
        Some(name) => Some(expression(name)),
        None => check_name(job.name.as_ref(), job.pr),
    }
}

fn expression(name: &SelectionName) -> String {
    let modes: Vec<String> = name
        .roots
        .iter()
        .map(|slug| {
            format!(
                "needs.{}.outputs.{}",
                name.plan,
                GateNeed::Selected.output_name(slug)
            )
        })
        .collect();
    let any = |selection: Selection| {
        modes
            .iter()
            .map(|mode| format!("{mode} == '{}'", selection.plan_value()))
            .collect::<Vec<_>>()
            .join(" || ")
    };
    let every = modes
        .iter()
        .map(|mode| format!("{mode} == '{}'", Selection::Nothing.plan_value()))
        .collect::<Vec<_>>()
        .join(" && ");
    format!(
        "${{{{ format('{BUDGET_JOB_NAME} ({{0}})', ({}) && '{}' || (({}) && '{}' || (({every}) && '{}' || 'unknown'))) }}}}",
        any(Selection::Full),
        Selection::Full.name(),
        any(Selection::Affected),
        Selection::Affected.name(),
        Selection::Nothing.name(),
    )
}
