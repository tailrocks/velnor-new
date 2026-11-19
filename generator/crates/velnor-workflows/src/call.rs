// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::check::Context;
use crate::error::{WorkflowsError, refuse};
use crate::expression::{Scope, check, is_identifier, whole_expression};
use crate::plan::{Call, CallTarget, CallValue, InputKind, Workflow};
use crate::raw::RawJob;

/// Refuses the keys that mean nothing on a job that calls a reusable workflow.
pub(crate) fn refuse_step_keys(raw: &RawJob) -> Result<(), WorkflowsError> {
    let id = &raw.id;
    let stray = [
        (!raw.step.is_empty(), "step"),
        (raw.helper, "helper"),
        (raw.environment.is_some(), "environment"),
        (!raw.env.is_empty(), "env"),
        (raw.cache_mode.is_some(), "cache_mode"),
        (raw.runner.is_some(), "runner"),
        (raw.timeout_minutes.is_some(), "timeout_minutes"),
        (!raw.outputs.is_empty(), "outputs"),
        (raw.container.is_some(), "container"),
        (!raw.service.is_empty(), "service"),
    ]
    .into_iter()
    .find_map(|(set, name)| set.then_some(name));
    match stray {
        Some(name) => refuse(format!(
            "job {id} calls a reusable workflow with uses, so it cannot set {name}; the called workflow runs its own jobs"
        )),
        None => Ok(()),
    }
}

pub(crate) fn call(
    id: &str,
    uses: &str,
    with: &BTreeMap<String, toml::Value>,
    permissions: &[(String, String)],
    ctx: &Context<'_>,
    scope: &Scope<'_>,
) -> Result<Call, WorkflowsError> {
    let (target, callee) = target(id, uses, ctx)?;
    fence_callee(id, uses, &target, ctx)?;
    let inputs = callee
        .map(|flow| match &flow.triggers.workflow_call {
            Some(call) => Ok(call),
            None => refuse(format!(
                "job {id} calls {}, which has no workflow_call trigger",
                flow.file
            )),
        })
        .transpose()?;
    let mut values = Vec::new();
    for (key, raw) in with {
        let kind = match inputs {
            Some(call) => Some(
                call.inputs
                    .iter()
                    .find(|input| &input.name == key)
                    .map(|input| &input.kind)
                    .ok_or_else(|| {
                        WorkflowsError(format!(
                            "job {id} passes {key}, which the called workflow does not declare as an input"
                        ))
                    })?,
            ),
            None => None,
        };
        if !is_identifier(key, true) {
            return refuse(format!("job {id} with key {key:?} is not an identifier"));
        }
        values.push((key.clone(), value(id, key, raw, kind, scope)?));
    }
    if let Some(call) = inputs
        && let Some(missing) = call
            .inputs
            .iter()
            .find(|input| input.required && !with.contains_key(&input.name))
    {
        return refuse(format!(
            "job {id} does not pass the required input {} of the called workflow",
            missing.name
        ));
    }
    if let Some(flow) = callee {
        within_nesting_limit(id, flow, ctx.workflows)?;
        grants_enough(id, flow, permissions)?;
    }
    Ok(Call {
        target,
        with: values,
        outputs: inputs
            .map(|call| call.outputs.iter().map(|out| out.name.clone()).collect())
            .unwrap_or_default(),
    })
}

fn fence_callee(
    id: &str,
    uses: &str,
    target: &CallTarget,
    ctx: &Context<'_>,
) -> Result<(), WorkflowsError> {
    let started_by_a_pull_request = ctx.triggers.pull_request.is_some() || ctx.triggers.merge_group;
    let outside = match target {
        CallTarget::Local(_) | CallTarget::Pinned(_) => true,
        CallTarget::Declared(file) => ctx
            .workflows
            .iter()
            .find(|flow| &flow.file == file)
            .is_some_and(|flow| calls_outside(flow, ctx.workflows)),
    };
    if started_by_a_pull_request && outside {
        return refuse(format!(
            "job {id} calls {uses}, which runs jobs this model cannot check or put behind the fork fence, from a workflow that a pull_request or merge_group event starts; call a workflow declared earlier in this file instead, or start this workflow from push, schedule, workflow_dispatch or workflow_call only"
        ));
    }
    Ok(())
}

fn calls_outside(flow: &Workflow, declared: &[Workflow]) -> bool {
    flow.jobs
        .iter()
        .filter_map(|job| job.call.as_ref())
        .any(|call| match &call.target {
            CallTarget::Local(_) | CallTarget::Pinned(_) => true,
            CallTarget::Declared(file) => declared
                .iter()
                .find(|other| &other.file == file)
                .is_some_and(|other| calls_outside(other, declared)),
        })
}

fn target<'a>(
    id: &str,
    uses: &str,
    ctx: &'a Context<'_>,
) -> Result<(CallTarget, Option<&'a Workflow>), WorkflowsError> {
    if let Some(name) = uses.strip_prefix("./.github/workflows/") {
        return if workflow_file(name) {
            Ok((CallTarget::Local(uses.to_owned()), None))
        } else {
            refuse(format!(
                "job {id} uses {uses:?}, which is not a workflow file under ./.github/workflows"
            ))
        };
    }
    if let Some((path, sha)) = uses.split_once('@') {
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        let pinned = sha.len() == 40
            && sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        let ok = matches!(parts.as_slice(), [owner, repo, rest]
            if name_part(owner)
                && name_part(repo)
                && rest.strip_prefix(".github/workflows/").is_some_and(workflow_file));
        return if ok && pinned {
            Ok((CallTarget::Pinned(uses.to_owned()), None))
        } else {
            refuse(format!(
                "job {id} uses {uses:?}; a workflow of another repository must be owner/repo/.github/workflows/<file>.yml@<full commit SHA>"
            ))
        };
    }
    match ctx.workflows.iter().find(|flow| flow.file == uses) {
        Some(flow) => Ok((CallTarget::Declared(uses.to_owned()), Some(flow))),
        None => refuse(format!(
            "job {id} uses {uses:?}, which is not a workflow declared earlier in the file, a ./.github/workflows file or a pinned owner/repo path"
        )),
    }
}

fn name_part(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn workflow_file(name: &str) -> bool {
    let stem = name
        .strip_suffix(".yml")
        .or_else(|| name.strip_suffix(".yaml"));
    stem.is_some_and(name_part)
}

fn value(
    id: &str,
    key: &str,
    raw: &toml::Value,
    kind: Option<&InputKind>,
    scope: &Scope<'_>,
) -> Result<CallValue, WorkflowsError> {
    let mismatch = |what: &str| {
        WorkflowsError(format!(
            "job {id} passes {key} as {what}, but the called workflow declares it as {}",
            kind.map_or("string", InputKind::name)
        ))
    };
    match raw {
        toml::Value::String(text) if text.contains("${{") => {
            check(text, scope)?;
            Ok(CallValue::Expression(
                whole_expression(text).unwrap_or_default().to_owned(),
            ))
        }
        toml::Value::String(text) => match kind {
            None | Some(InputKind::String | InputKind::Environment | InputKind::Choice(_)) => {
                Ok(CallValue::Text(text.clone()))
            }
            Some(InputKind::Boolean) => match text.as_str() {
                "true" => Ok(CallValue::Boolean(true)),
                "false" => Ok(CallValue::Boolean(false)),
                _ => Err(mismatch("text")),
            },
            Some(InputKind::Number) => text
                .parse::<u64>()
                .map(CallValue::Number)
                .map_err(|_| mismatch("text")),
        },
        toml::Value::Boolean(flag) if matches!(kind, None | Some(InputKind::Boolean)) => {
            Ok(CallValue::Boolean(*flag))
        }
        toml::Value::Integer(number) if matches!(kind, None | Some(InputKind::Number)) => {
            u64::try_from(*number)
                .map(CallValue::Number)
                .map_err(|_| mismatch("a negative number"))
        }
        _ => Err(mismatch("another type")),
    }
}

const MAX_NESTING_LEVELS: usize = 10;

fn levels(flow: &Workflow, declared: &[Workflow]) -> usize {
    1 + flow
        .jobs
        .iter()
        .filter_map(|job| job.call.as_ref())
        .filter_map(|call| match &call.target {
            CallTarget::Declared(file) => declared.iter().find(|other| &other.file == file),
            CallTarget::Local(_) | CallTarget::Pinned(_) => None,
        })
        .map(|other| levels(other, declared))
        .max()
        .unwrap_or(0)
}

fn within_nesting_limit(
    id: &str,
    callee: &Workflow,
    declared: &[Workflow],
) -> Result<(), WorkflowsError> {
    let chain = 1 + levels(callee, declared);
    if chain > MAX_NESTING_LEVELS {
        return refuse(format!(
            "job {id} calls {}, which makes a chain of {chain} workflows; GitHub allows {MAX_NESTING_LEVELS} levels, the top-level workflow and nine reusable ones",
            callee.file
        ));
    }
    Ok(())
}

pub(crate) fn rank(access: &str) -> u8 {
    match access {
        "write" => 2,
        "read" => 1,
        _ => 0,
    }
}

/// A caller job cannot grant a called workflow more than it holds, and a
/// called job that asks for more than that makes the whole run fail to start.
fn grants_enough(
    id: &str,
    callee: &Workflow,
    granted: &[(String, String)],
) -> Result<(), WorkflowsError> {
    for job in &callee.jobs {
        for (scope, access) in &job.permissions {
            let held = granted
                .iter()
                .find(|(name, _)| name == scope)
                .map_or(0, |(_, level)| rank(level));
            if rank(access) > held {
                return refuse(format!(
                    "job {id} calls {}, whose job {} needs {scope} = {access:?}; give the calling job at least that permission",
                    callee.file, job.id
                ));
            }
        }
    }
    Ok(())
}
