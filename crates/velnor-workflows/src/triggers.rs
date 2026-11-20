// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CronExpr;

use crate::error::{WorkflowsError, refuse};
use crate::plan::{PullRequest, Push, Triggers, WorkflowCall, WorkflowRun};
use crate::raw::{RawInput, RawOn, RawOutput, RawPullRequest, RawPush, RawWorkflowRun};

mod inputs;
mod patterns;

use inputs::{call_output, inputs};
use patterns::{branch_patterns, filter_patterns, names, path_filters, workflow_names};

const PULL_REQUEST_TYPES: [&str; 21] = [
    "assigned",
    "unassigned",
    "labeled",
    "unlabeled",
    "opened",
    "edited",
    "closed",
    "reopened",
    "synchronize",
    "converted_to_draft",
    "locked",
    "unlocked",
    "enqueued",
    "dequeued",
    "milestoned",
    "demilestoned",
    "ready_for_review",
    "review_requested",
    "review_request_removed",
    "auto_merge_enabled",
    "auto_merge_disabled",
];
const MIN_SCHEDULE_GAP_MINUTES: u32 = 5;
const WORKFLOW_RUN_TYPES: [&str; 3] = ["completed", "requested", "in_progress"];

/// Reads `[workflow.on]` and the call inputs and outputs. Without an `on`
/// table the workflow is a reusable workflow, which is what the table `file`
/// plus `[[workflow.input]]` always meant.
pub(crate) fn triggers(
    file: &str,
    on: Option<RawOn>,
    inputs: Vec<RawInput>,
    outputs: Vec<RawOutput>,
) -> Result<Triggers, WorkflowsError> {
    let on = on.unwrap_or(RawOn {
        workflow_call: Some(true),
        ..RawOn::default()
    });
    let mut triggers = Triggers::default();
    if on.workflow_call == Some(true) {
        triggers.workflow_call = Some(WorkflowCall {
            inputs: self::inputs(file, inputs, false)?,
            outputs: outputs
                .into_iter()
                .map(|output| call_output(file, output))
                .collect::<Result<_, _>>()?,
        });
    } else if !inputs.is_empty() || !outputs.is_empty() {
        return refuse(format!(
            "the workflow {file} declares [[workflow.input]] or [[workflow.output]], which belong to the workflow_call trigger; set workflow_call = true under [workflow.on]"
        ));
    }
    if let Some(dispatch) = on.workflow_dispatch {
        triggers.workflow_dispatch = Some(self::inputs(file, dispatch.input, true)?);
    }
    for cron in on.schedule {
        let parsed = CronExpr::new(cron.clone())
            .ok()
            .filter(|_| !triggers.schedule.contains(&cron));
        let Some(parsed) = parsed else {
            return refuse(format!(
                "the schedule {cron:?} of {file} is not a unique five-field cron expression"
            ));
        };
        let Some(gap) = parsed.shortest_gap_minutes() else {
            return refuse(format!(
                "the schedule {cron:?} of {file} never runs, because no day matches its day, month and weekday fields"
            ));
        };
        if gap < MIN_SCHEDULE_GAP_MINUTES {
            return refuse(format!(
                "the schedule {cron:?} of {file} runs more often than every {MIN_SCHEDULE_GAP_MINUTES} minutes, which is the shortest interval GitHub runs a schedule at; use at least 5 minutes between runs"
            ));
        }
        triggers.schedule.push(cron);
    }
    filtered(
        file,
        &mut triggers,
        on.push,
        on.pull_request,
        on.workflow_run,
    )?;
    triggers.merge_group = on.merge_group == Some(true);
    let any = triggers.workflow_call.is_some()
        || triggers.workflow_dispatch.is_some()
        || !triggers.schedule.is_empty()
        || triggers.push.is_some()
        || triggers.pull_request.is_some()
        || triggers.merge_group
        || triggers.workflow_run.is_some();
    if !any {
        return refuse(format!("the workflow {file} needs at least one trigger"));
    }
    Ok(triggers)
}

fn filtered(
    file: &str,
    triggers: &mut Triggers,
    push: Option<RawPush>,
    request: Option<RawPullRequest>,
    run: Option<RawWorkflowRun>,
) -> Result<(), WorkflowsError> {
    if let Some(push) = push {
        if push.branches.is_empty() && push.tags.is_empty() {
            return refuse(format!(
                "the push trigger of {file} needs branches or tags; an unfiltered push trigger is refused"
            ));
        }
        let (paths, paths_ignore) = path_filters(file, "push", push.paths, push.paths_ignore)?;
        triggers.push = Some(Push {
            branches: branch_patterns(file, "push branches", push.branches)?,
            tags: filter_patterns(file, "push tags", push.tags)?,
            paths,
            paths_ignore,
        });
    }
    if let Some(request) = request {
        let (paths, paths_ignore) =
            path_filters(file, "pull_request", request.paths, request.paths_ignore)?;
        triggers.pull_request = Some(PullRequest {
            types: names(
                file,
                "pull_request types",
                request.types,
                &PULL_REQUEST_TYPES,
            )?,
            branches: branch_patterns(file, "pull_request branches", request.branches)?,
            paths,
            paths_ignore,
        });
        if triggers
            .pull_request
            .as_ref()
            .is_some_and(|request| request.types.is_empty())
        {
            return refuse(format!(
                "the pull_request trigger of {file} needs types, for example [\"opened\", \"synchronize\"]"
            ));
        }
    }
    if let Some(run) = run {
        let types = if run.types.is_empty() {
            vec!["completed".to_owned()]
        } else {
            names(file, "workflow_run types", run.types, &WORKFLOW_RUN_TYPES)?
        };
        if run.workflows.is_empty() {
            return refuse(format!(
                "the workflow_run trigger of {file} needs the names of the workflows it follows"
            ));
        }
        triggers.workflow_run = Some(WorkflowRun {
            workflows: workflow_names(file, run.workflows)?,
            types,
            branches: branch_patterns(file, "workflow_run branches", run.branches)?,
        });
    }
    Ok(())
}
