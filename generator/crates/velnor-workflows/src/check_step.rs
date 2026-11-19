// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::{WorkflowsError, refuse};
use crate::expression::{
    Scope, WITH_INTERPOLATIONS, check, check_condition, check_interpolated, is_identifier,
    whole_expression,
};
use crate::helper_call::{HELPER_COMMANDS, calls_velnor_ci, subcommand};
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
        (None, Some(name), None) if is_local(&name) => local_action(job, name)?,
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
    let raw_with = locked_inputs(job, &body, &label, raw.with)?;
    let condition = raw
        .condition
        .map(|text| check_condition(&text, scope))
        .transpose()?;
    Ok(Step {
        id: raw.id,
        name: raw.name.unwrap_or(label),
        condition,
        body,
        with: values(job, raw_with, scope, Some(&WITH_INTERPOLATIONS))?,
        continue_on_error: raw.continue_on_error,
        env: values(job, raw.env, scope, None)?,
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

fn local_action(job: &str, name: String) -> Result<(String, Body), WorkflowsError> {
    let Some(action) = LocalAction::parse(&name) else {
        return refuse(format!(
            "job {job} uses the local action {name:?}, which is not allowed; the only local actions are {:?}, and the pinned actions are {:?}",
            LocalAction::PATHS,
            Action::NAMES
        ));
    };
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

fn values(
    job: &str,
    table: BTreeMap<String, String>,
    scope: &Scope<'_>,
    interpolations: Option<&[&str]>,
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
            check(&value, scope)?;
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
