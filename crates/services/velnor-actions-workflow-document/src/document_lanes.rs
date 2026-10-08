//! Lane jobs calling shared composite actions.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{Job, Step, StepKind};

use crate::document_steps::step_to_yaml;
use crate::lane_share::{SharedActionCall, SharedActionInput};
use velnor_actions_workflow_jobs::RenderContext;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::composite::{shared_call, shared_call_named_with_inputs};
use velnor_actions_workflow_tree::rendered::RenderedFile;
use velnor_actions_workflow_tree::yaml::Yaml;
use velnor_actions_workflow_tree::{marker, yaml::render_yaml};

pub(crate) struct SharedLaneSteps<'a> {
    pub checkouts: &'a BTreeMap<String, Step>,
    pub env_steps: &'a BTreeMap<String, Vec<Step>>,
    pub prefixes: &'a BTreeMap<String, Vec<Step>>,
    pub preludes: &'a BTreeMap<String, Vec<Step>>,
    pub postludes: &'a BTreeMap<String, Vec<Step>>,
}

pub(crate) struct JobStepContext<'a> {
    pub job_env: &'a BTreeMap<String, String>,
    pub actions_read: bool,
}
/// Render a normal job body or a paired lane's cache prelude/composite/postlude.
pub(crate) fn render_job_steps(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: Option<&SharedActionCall>,
    lanes: &SharedLaneSteps<'_>,
    step_context: &JobStepContext<'_>,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered = Vec::with_capacity(job.steps.len() + 2 * usize::from(shared.is_some()));
    if let Some(call) = shared {
        append_shared_lane_steps(
            id,
            call,
            ctx,
            needs_envs,
            lanes,
            step_context,
            &mut rendered,
        )?;
    } else {
        if lanes.checkouts.contains_key(id) {
            return Err(RenderError::InvalidWorkflow(format!(
                "checkout_without_shared_lane:{id}"
            )));
        }
        for step in &job.steps {
            rendered.push(step_to_yaml(
                id,
                step,
                ctx,
                needs_envs,
                false,
                step_context.job_env,
                step_context.actions_read,
            )?);
        }
    }
    Ok(rendered)
}

fn append_shared_lane_steps(
    id: &str,
    call: &SharedActionCall,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    lanes: &SharedLaneSteps<'_>,
    step_context: &JobStepContext<'_>,
    rendered: &mut Vec<Yaml>,
) -> Result<(), RenderError> {
    let Some(checkout) = lanes.checkouts.get(id) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "shared_lane_missing_checkout:{id}"
        )));
    };
    if !crate::lane_share_sections::valid_shared_checkout(checkout, &ctx.checkout_uses) {
        return Err(RenderError::InvalidWorkflow(format!(
            "shared_lane_invalid_checkout:{id}"
        )));
    }
    rendered.push(step_to_yaml(
        id,
        checkout,
        ctx,
        needs_envs,
        false,
        step_context.job_env,
        step_context.actions_read,
    )?);
    crate::lane_share_sections::append_steps(
        id,
        lanes.prefixes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "prefix",
    )?;
    crate::lane_share_sections::append_steps(
        id,
        lanes.preludes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "prelude",
    )?;
    rendered.push(shared_action_call(call)?);
    crate::lane_share_sections::append_steps(
        id,
        lanes.postludes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "postlude",
    )
}

fn shared_action_call(call: &SharedActionCall) -> Result<Yaml, RenderError> {
    if call.inputs.is_empty() {
        return shared_call(&call.uses);
    }
    let mut seen = BTreeSet::new();
    let inputs = call
        .inputs
        .iter()
        .map(|input| {
            let (name, value) = match input {
                SharedActionInput::CoveredTasks => {
                    ("covered_tasks", "${{ needs.plan.outputs.covered_tasks }}")
                }
            };
            if !seen.insert(name) {
                return Err(RenderError::InvalidWorkflow(
                    "shared_action_duplicate_input".to_owned(),
                ));
            }
            Ok((name.to_owned(), Yaml::str(value)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    shared_call_named_with_inputs(&call.uses, "Run shared steps", inputs)
}

fn task_composite_file(
    logical: &str,
    source: &[Step],
    postlude: &[Step],
    ctx: &RenderContext,
) -> Result<(RenderedFile, Vec<SharedActionInput>), RenderError> {
    let mut body = source.to_vec();
    let mut covered_task_conditions = BTreeSet::new();
    for step in &mut body {
        if let Some(condition) = &mut step.condition
            && condition.contains("needs.")
        {
            let task_id = crate::lane_share::task_coverage_id(condition).ok_or_else(|| {
                crate::lane_share_sections::task_factor_error(logical, "unsupported_condition")
            })?;
            *condition = format!("!contains(inputs.covered_tasks, ',{task_id},')");
            covered_task_conditions.insert(condition.clone());
        }
    }
    if covered_task_conditions.is_empty() {
        return Err(crate::lane_share_sections::task_factor_error(
            logical,
            "plan_gate_missing",
        ));
    }
    validate_composite_task_scope(&body, &covered_task_conditions)?;
    let ids = body
        .iter()
        .filter_map(|step| step.id.map(|id| id.as_str().to_owned()))
        .collect();
    if postlude
        .iter()
        .any(|step| references_any_step_output(step, &ids))
    {
        return Err(crate::lane_share_sections::task_factor_error(
            logical,
            "postlude_output_scope",
        ));
    }
    velnor_actions_contract_workflow::workflow::step_identity::validate_step_identity_scope(
        &body,
        &format!("composite:{logical}"),
    )
    .map_err(RenderError::Contract)?;
    let empty_env = BTreeMap::new();
    let rendered = body
        .iter()
        .map(|step| step_to_yaml(logical, step, ctx, &[], true, &empty_env, false))
        .collect::<Result<Vec<_>, _>>()?;
    let input = (
        "covered_tasks".to_owned(),
        Yaml::Map(vec![
            (
                "description".to_owned(),
                Yaml::str("Task IDs already covered by the plan"),
            ),
            ("required".to_owned(), Yaml::Bool(true)),
        ]),
    );
    let action = velnor_actions_workflow_tree::composite::composite_yaml_with_inputs(
        logical,
        vec![input],
        rendered,
    )?;
    let quoted = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(action);
    let bytes = marker::with_marker(&ctx.generator_version, &render_yaml(&quoted))?;
    velnor_actions_workflow_steps::steps::scan_for_private_subcommands(&bytes)?;
    Ok((
        RenderedFile {
            path: format!(".github/actions/{logical}/action.yml"),
            bytes,
        },
        vec![SharedActionInput::CoveredTasks],
    ))
}

fn validate_composite_task_scope(
    steps: &[Step],
    covered_task_conditions: &BTreeSet<String>,
) -> Result<(), RenderError> {
    let mut prior = BTreeSet::new();
    for step in steps {
        for value in step_values(step) {
            if value.contains("needs.")
                || (value.contains("inputs.") && !covered_task_conditions.contains(value))
                || value.contains("matrix.")
                || value.contains("strategy.")
                || ["needs", "inputs", "matrix", "strategy", "steps"]
                    .into_iter()
                    .any(|context| contains_indexed_context(value, context))
                || !valid_step_output_scope(value, &prior)
            {
                return Err(crate::lane_share_sections::task_factor_error(
                    &step.name,
                    "unsupported_context",
                ));
            }
        }
        if let Some(id) = step.id {
            prior.insert(id.as_str().to_owned());
        }
    }
    Ok(())
}

fn valid_step_output_scope(value: &str, prior: &BTreeSet<String>) -> bool {
    let mut remaining = value;
    while let Some((_, after)) = remaining.split_once("steps.") {
        let Some((id, tail)) = after.split_once(".outputs.") else {
            return false;
        };
        if !prior.contains(id) {
            return false;
        }
        remaining = tail;
    }
    true
}

pub(crate) fn references_any_step_output(step: &Step, ids: &BTreeSet<String>) -> bool {
    step_values(step).into_iter().any(|value| {
        if contains_indexed_context(value, "steps") {
            return true;
        }
        let mut remaining = value;
        while let Some((_, after)) = remaining.split_once("steps.") {
            let Some((id, tail)) = after.split_once(".outputs.") else {
                return true;
            };
            if ids.contains(id) {
                return true;
            }
            remaining = tail;
        }
        false
    })
}

pub(crate) fn contains_indexed_context(value: &str, context: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = value[offset..].find(context) {
        let start = offset + relative;
        let end = start + context.len();
        let left_boundary = start == 0 || !is_context_name_byte(value.as_bytes()[start - 1]);
        let mut after = end;
        while value
            .as_bytes()
            .get(after)
            .is_some_and(u8::is_ascii_whitespace)
        {
            after += 1;
        }
        if left_boundary && value.as_bytes().get(after) == Some(&b'[') {
            return true;
        }
        offset = end;
        if offset >= value.len() {
            return false;
        }
    }
    false
}

fn is_context_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn step_values(step: &Step) -> Vec<&str> {
    let mut values = vec![step.name.as_str()];
    values.extend(step.condition.iter().map(String::as_str));
    match &step.kind {
        StepKind::Shell { run, env } => {
            values.extend(run.iter().map(String::as_str));
            values.extend(
                env.iter()
                    .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
            );
        }
        StepKind::Action { uses, with, env } => {
            values.push(uses);
            values.extend(
                with.iter()
                    .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
            );
            values.extend(
                env.iter()
                    .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
            );
        }
        StepKind::Internal { operation, env } => {
            values.push(operation);
            values.extend(
                env.iter()
                    .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
            );
        }
    }
    values
}

#[path = "document_task_lanes.rs"]
mod task_lanes;

pub(crate) use task_lanes::factor_task_report_jobs;
