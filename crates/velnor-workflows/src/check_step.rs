// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::{WorkflowsError, refuse};
use crate::expression::{
    Scope, WITH_INTERPOLATIONS, check, check_condition, check_env, check_interpolated,
    is_identifier, whole_expression,
};
use crate::helper_call::{HELPER_COMMANDS, calls_velnor_ci, subcommand};
use crate::job_limits::bounded_cache_keys;
use crate::plan::{Action, Body, Env, LocalAction, Step};
use crate::raw::RawStep;

pub(crate) fn step(
    job: &str,
    helper_installed: bool,
    raw: RawStep,
    scope: &Scope<'_>,
    earlier: &[Step],
) -> Result<Step, WorkflowsError> {
    if let Some(id) = &raw.id
        && (!is_identifier(id, true) || earlier.iter().any(|seen| seen.id.as_ref() == Some(id)))
    {
        return refuse(format!(
            "job {job} step id {id:?} is not a unique identifier"
        ));
    }
    let (label, body) = match (raw.run, raw.uses, raw.helper) {
        (Some(run), None, None) => {
            if raw
                .name
                .as_deref()
                .is_none_or(|name| name.trim().is_empty())
                || run.trim().is_empty()
            {
                return refuse(format!("a run step of job {job} needs a name and a run"));
            }
            if !helper_installed && calls_velnor_ci(&run) {
                return refuse(format!(
                    "job {job} runs velnor-ci, so it needs helper = true"
                ));
            }
            if calls_velnor_ci(&run) && !run.starts_with("velnor-ci ") {
                return refuse(format!(
                    "job {job} calls velnor-ci, which must start with velnor-ci as the command word"
                ));
            }
            if calls_velnor_ci(&run) {
                known_command(job, subcommand(&run).as_deref().unwrap_or_default())?;
            }
            (String::new(), Body::Run(run))
        }
        (None, Some(name), None) if is_local(&name) => local_action(job, name, earlier)?,
        (None, Some(name), None) => {
            let Some(action) = Action::parse(&name) else {
                return refuse(format!(
                    "job {job} uses {name:?}, which is not one of the pinned actions {:?}",
                    Action::NAMES
                ));
            };
            (name, Body::Uses(action))
        }
        (None, None, Some(command)) => {
            if !is_identifier(&command, true) {
                return refuse(format!("job {job} helper {command:?} is not an identifier"));
            }
            if !helper_installed {
                return refuse(format!(
                    "job {job} runs the helper command {command}, so it needs helper = true"
                ));
            }
            known_command(job, &command)?;
            (command.clone(), Body::Helper(command))
        }
        _ => {
            return refuse(format!(
                "a step of job {job} needs exactly one of run, uses and helper"
            ));
        }
    };
    if !raw.with.is_empty() && !matches!(body, Body::Uses(_)) {
        return refuse(format!("a step of job {job} sets with without uses"));
    }
    declared_inputs(job, &body, &label, &raw.with)?;
    plain_script(job, &body, &raw.with)?;
    let raw_with = locked_inputs(job, &body, &label, raw.with)?;
    let condition = raw
        .condition
        .map(|text| check_condition(&text, scope))
        .transpose()?;
    let working_directory = working_directory(job, &body, raw.working_directory)?;
    let with = values(job, raw_with, scope, Some(WITH_INTERPOLATIONS), check)?;
    bounded_cache_keys(job, &body, &with)?;
    Ok(Step {
        id: raw.id,
        name: raw.name.unwrap_or(label),
        condition,
        body,
        with,
        continue_on_error: raw.continue_on_error,
        working_directory,
        env: values(job, raw.env, scope, None, check_env)?,
    })
}

fn working_directory(
    job: &str,
    body: &Body,
    directory: Option<String>,
) -> Result<Option<String>, WorkflowsError> {
    let Some(directory) = directory else {
        return Ok(None);
    };
    if !matches!(body, Body::Run(_)) {
        return refuse(format!(
            "job {job} sets working_directory on a step that is not a run step"
        ));
    }
    let inside = !directory.starts_with(['/', '~'])
        && !directory.contains(|c: char| c.is_control() || matches!(c, '$' | '\\' | '`' | ':'))
        && directory
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if !inside {
        return refuse(format!(
            "job {job} working_directory {directory:?} must be a plain relative path inside the workspace: no leading / or ~, no empty, . or .. part, no expression, $, backslash, backtick or colon"
        ));
    }
    Ok(Some(directory))
}

fn plain_script(
    job: &str,
    body: &Body,
    with: &BTreeMap<String, String>,
) -> Result<(), WorkflowsError> {
    if !matches!(body, Body::Uses(Action::GithubScript)) {
        return Ok(());
    }
    let Some((_, script)) = with
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("script"))
    else {
        return Ok(());
    };
    if is_log_line(script) {
        return Ok(());
    }
    refuse(format!(
        "job {job} sets the github-script script {script:?}, which is not allowed; a script is exactly one console.log('<plain text>') statement, because decisions and loops live in the helper"
    ))
}

fn is_log_line(script: &str) -> bool {
    let script = script.trim();
    let script = script.strip_suffix(';').unwrap_or(script);
    let Some(argument) = script
        .strip_prefix("console.log(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    ['\'', '"'].into_iter().any(|quote| {
        argument
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
            .is_some_and(|text| {
                !text.contains(|c: char| {
                    c == quote || c.is_control() || matches!(c, '\\' | '`' | '$')
                })
            })
    })
}

fn known_command(job: &str, command: &str) -> Result<(), WorkflowsError> {
    if HELPER_COMMANDS.contains(&command) {
        return Ok(());
    }
    refuse(format!(
        "job {job} runs velnor-ci {command:?}, which the pinned velnor-ci does not have; its commands are {HELPER_COMMANDS:?}"
    ))
}

fn declared_inputs(
    job: &str,
    body: &Body,
    label: &str,
    with: &BTreeMap<String, String>,
) -> Result<(), WorkflowsError> {
    let Body::Uses(action) = body else {
        return Ok(());
    };
    let known = |name: &str| {
        action
            .known_inputs()
            .iter()
            .any(|input| input.eq_ignore_ascii_case(name))
    };
    for (index, key) in with.keys().enumerate() {
        let repeated = with
            .keys()
            .take(index)
            .any(|earlier| earlier.eq_ignore_ascii_case(key));
        if !known(key) || repeated {
            return refuse(format!(
                "job {job} sets {key:?} on {label}, which is not a unique input of that action; its inputs are {:?}",
                action.known_inputs()
            ));
        }
    }
    for needed in action.required_inputs() {
        if !with.keys().any(|key| key.eq_ignore_ascii_case(needed)) {
            return refuse(format!(
                "job {job} does not set the required input {needed} of {label}"
            ));
        }
    }
    Ok(())
}

fn checks_out_workspace(step: &Step) -> bool {
    matches!(step.body, Body::Uses(Action::Checkout))
        && step.condition.is_none()
        && !step.continue_on_error
        && !step
            .with
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("path"))
}

fn local_action(
    job: &str,
    name: String,
    earlier: &[Step],
) -> Result<(String, Body), WorkflowsError> {
    let Some(action) = LocalAction::parse(&name) else {
        return refuse(format!(
            "job {job} uses the local action {name:?}, which is not allowed; the only local actions are {:?}, and the pinned actions are {:?}",
            LocalAction::PATHS,
            Action::NAMES
        ));
    };
    if !earlier.iter().any(checks_out_workspace) {
        return refuse(format!(
            "job {job} uses the local action {name:?} without an earlier unconditional checkout step into the default workspace; a local action is read from the checked out repository"
        ));
    }
    Ok((name, Body::Local(action)))
}

fn is_local(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('/') || name.contains('/')
}

fn locked_inputs(
    job: &str,
    body: &Body,
    label: &str,
    mut with: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, WorkflowsError> {
    let Body::Uses(action) = body else {
        return Ok(with);
    };
    for (key, value) in &with {
        if action
            .locked_off()
            .iter()
            .any(|locked| key.eq_ignore_ascii_case(locked))
            && value != "false"
        {
            return refuse(format!(
                "job {job} sets {key} = {value:?} on {label}, which is always false"
            ));
        }
    }
    with.retain(|key, _| {
        !action
            .locked_off()
            .iter()
            .any(|locked| key.eq_ignore_ascii_case(locked))
    });
    Ok(with)
}

/// `checker` is `check` for `with` and `check_env` for `env`, the one place a secret may be read.
fn values(
    job: &str,
    table: BTreeMap<String, String>,
    scope: &Scope<'_>,
    interpolations: Option<&[&str]>,
    checker: fn(&str, &Scope<'_>) -> Result<(), WorkflowsError>,
) -> Result<Vec<(String, Env)>, WorkflowsError> {
    let mut items = Vec::new();
    for (key, value) in table {
        if !is_identifier(&key, true) {
            return refuse(format!("job {job} key {key:?} is not an identifier"));
        }
        let whole = whole_expression(&value);
        let interpolated = interpolations.filter(|allowed| {
            value.contains("${{") && whole.is_none_or(|inner| allowed.contains(&inner))
        });
        if let Some(allowed) = interpolated {
            check_interpolated(&format!("job {job} with {key}"), &value, allowed)?;
            items.push((key, Env::Interpolated(value)));
        } else if value.contains("${{") {
            checker(&value, scope)?;
            items.push((key, Env::Expression(whole_inner(&value))));
        } else {
            items.push((key, Env::Text(value)));
        }
    }
    Ok(items)
}

fn whole_inner(value: &str) -> String {
    whole_expression(value).unwrap_or_default().to_owned()
}
