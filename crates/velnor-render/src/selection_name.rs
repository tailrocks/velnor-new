// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BUDGET_JOB_NAME, GateNeed, Job, Selection, SelectionName};

use crate::event_gate::event_gate;
use crate::gate::{ACTIVE_EVENT, Setup, check_name, merge_group_skipped, pr_parts};

/// The expression for the title of a job, when it needs one: the title of the
/// job that records the selection of its run, or the title of a job whose check
/// the pull request events may ignore.
pub(crate) fn title(job: &Job) -> Option<String> {
    match &job.selection_name {
        Some(name) => Some(expression(name, job)),
        None => check_name(job.name.as_ref(), job.pr),
    }
}

fn guard(job: &Job) -> String {
    let mut parts = vec![ACTIVE_EVENT.to_owned(), event_gate(job.when)];
    parts.extend(crate::gate_needs::succeeded(job.when, &job.needs));
    parts.extend(
        job.skip_after_merge_group
            .as_ref()
            .map(|from| merge_group_skipped(from, Setup::Before)),
    );
    parts.extend(pr_parts(job.pr));
    parts.join(" && ")
}

fn expression(name: &SelectionName, job: &Job) -> String {
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
    let ran = guard(job);
    format!(
        "${{{{ ({ran}) && format('{BUDGET_JOB_NAME} ({{0}})', ({}) && '{}' || (({}) && '{}' || (({every}) && '{}' || 'unknown'))) || '{BUDGET_JOB_NAME}' }}}}",
        any(Selection::Full),
        Selection::Full.name(),
        any(Selection::Affected),
        Selection::Affected.name(),
        Selection::Nothing.name(),
    )
}
