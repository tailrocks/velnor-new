// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CronExpr;

use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{
    Input, InputKind, Output, PullRequest, Push, Triggers, WorkflowCall, WorkflowRun,
};
use crate::raw::{RawInput, RawOn, RawOutput, RawPullRequest, RawPush, RawWorkflowRun};

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
const EVERY_PATH: &str = "**";
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
        if parsed.shortest_gap_minutes() < MIN_SCHEDULE_GAP_MINUTES {
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
            tags: patterns(file, "push tags", push.tags)?,
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

fn names(
    file: &str,
    what: &str,
    values: Vec<String>,
    allowed: &[&str],
) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        if !allowed.contains(&value.as_str()) || values[..index].contains(value) {
            return refuse(format!(
                "{what} of {file} has {value:?}, which is not a unique one of {allowed:?}"
            ));
        }
    }
    Ok(values)
}

fn workflow_names(file: &str, values: Vec<String>) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        let bad = value.trim().is_empty()
            || value.contains(char::is_control)
            || value.contains("${{")
            || values[..index].contains(value);
        if bad {
            return refuse(format!(
                "workflow_run workflows of {file} has {value:?}, which is not a unique one-line workflow name without expressions"
            ));
        }
    }
    Ok(values)
}

fn branch_patterns(
    file: &str,
    what: &str,
    values: Vec<String>,
) -> Result<Vec<String>, WorkflowsError> {
    if values.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{what} of {file} has {EVERY_PATH:?}, which is redundant; omit branches to match every branch"
        ));
    }
    patterns(file, what, values)
}

type PathFilters = (Vec<String>, Vec<String>);

fn path_filters(
    file: &str,
    event: &str,
    paths: Vec<String>,
    ignored: Vec<String>,
) -> Result<PathFilters, WorkflowsError> {
    if !paths.is_empty() && !ignored.is_empty() {
        return refuse(format!(
            "the {event} trigger of {file} sets both paths and paths_ignore; GitHub allows one of them per trigger"
        ));
    }
    if paths.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{event} paths of {file} has {EVERY_PATH:?}, which is redundant; omit paths to run on every change"
        ));
    }
    if ignored.iter().any(|value| value == EVERY_PATH) {
        return refuse(format!(
            "{event} paths_ignore of {file} has {EVERY_PATH:?}, which ignores every file, so the workflow could never run"
        ));
    }
    Ok((
        patterns(file, &format!("{event} paths"), paths)?,
        patterns(file, &format!("{event} paths_ignore"), ignored)?,
    ))
}

fn patterns(file: &str, what: &str, values: Vec<String>) -> Result<Vec<String>, WorkflowsError> {
    for (index, value) in values.iter().enumerate() {
        let bad = value.is_empty()
            || value.contains(char::is_whitespace)
            || value.contains(char::is_control)
            || value.contains("${{")
            || values[..index].contains(value);
        if bad {
            return refuse(format!(
                "{what} of {file} has {value:?}, which is not a unique pattern without spaces or expressions"
            ));
        }
    }
    Ok(values)
}

fn inputs(file: &str, raw: Vec<RawInput>, dispatch: bool) -> Result<Vec<Input>, WorkflowsError> {
    let mut inputs: Vec<Input> = Vec::new();
    for input in raw {
        if !is_identifier(&input.name, true) || inputs.iter().any(|seen| seen.name == input.name) {
            return refuse(format!(
                "the input {:?} of {file} is not a unique identifier",
                input.name
            ));
        }
        inputs.push(one_input(file, input, dispatch)?);
    }
    Ok(inputs)
}

fn one_input(file: &str, input: RawInput, dispatch: bool) -> Result<Input, WorkflowsError> {
    let name = input.name;
    let kind = match (input.kind.as_deref().unwrap_or("string"), dispatch) {
        ("string", _) => InputKind::String,
        ("boolean", _) => InputKind::Boolean,
        ("number", _) => InputKind::Number,
        ("choice", true) if !input.options.is_empty() => {
            if input
                .options
                .iter()
                .enumerate()
                .any(|(index, option)| option.is_empty() || input.options[..index].contains(option))
            {
                return refuse(format!(
                    "the choice input {name} of {file} needs unique, non-empty options"
                ));
            }
            InputKind::Choice(input.options.clone())
        }
        ("environment", true) => InputKind::Environment,
        (other, _) => {
            return refuse(format!(
                "the input {name} of {file} has type {other:?}, which is not allowed here (a call input is a string, boolean or number; a dispatch input may also be a choice with options, or an environment)"
            ));
        }
    };
    if !matches!(kind, InputKind::Choice(_)) && !input.options.is_empty() {
        return refuse(format!(
            "the input {name} of {file} sets options, which only a choice input takes"
        ));
    }
    let default = input
        .default
        .map(|value| default_text(file, &name, &kind, &value))
        .transpose()?;
    Ok(Input {
        required: input.required.unwrap_or(default.is_none()),
        description: input.description,
        name,
        kind,
        default,
    })
}

fn default_text(
    file: &str,
    name: &str,
    kind: &InputKind,
    value: &toml::Value,
) -> Result<String, WorkflowsError> {
    let text = match (kind, value) {
        (InputKind::Boolean, toml::Value::Boolean(flag)) => Some(flag.to_string()),
        (InputKind::Number, toml::Value::Integer(number)) if *number >= 0 => {
            Some(number.to_string())
        }
        (InputKind::String | InputKind::Environment, toml::Value::String(text)) => {
            Some(text.clone())
        }
        (InputKind::Choice(options), toml::Value::String(text)) if options.contains(text) => {
            Some(text.clone())
        }
        _ => None,
    };
    text.ok_or_else(|| {
        WorkflowsError(format!(
            "the default of input {name} of {file} is not a {} value (a choice default must be one of its options)",
            kind.name()
        ))
    })
}

fn call_output(file: &str, raw: RawOutput) -> Result<Output, WorkflowsError> {
    let Some((job, output)) = raw.value.split_once('.') else {
        return refuse(format!(
            "the output {} of {file} must have value <job>.<output>, got {:?}",
            raw.name, raw.value
        ));
    };
    if !is_identifier(&raw.name, true) || !is_identifier(job, true) || !is_identifier(output, true)
    {
        return refuse(format!(
            "the output {} of {file} must be an identifier with value <job>.<output>",
            raw.name
        ));
    }
    Ok(Output {
        description: raw.description,
        from_job: job.to_owned(),
        from_output: output.to_owned(),
        name: raw.name,
    })
}
