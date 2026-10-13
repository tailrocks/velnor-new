use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};

use crate::document_steps::step_to_yaml;
use crate::render::RenderContext;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::composite::shared_call;
use velnor_actions_workflow_tree::yaml::Yaml;

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
    shared: Option<&str>,
    lanes: &SharedLaneSteps<'_>,
    step_context: &JobStepContext<'_>,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered = Vec::with_capacity(job.steps.len() + 2 * usize::from(shared.is_some()));
    if let Some(uses) = shared {
        append_shared_lane_steps(
            id,
            uses,
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
    uses: &str,
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
    if !valid_shared_checkout(checkout, &ctx.checkout_uses) {
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
    append_steps(
        id,
        lanes.prefixes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "prefix",
    )?;
    append_steps(
        id,
        lanes.preludes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "prelude",
    )?;
    rendered.push(shared_call(uses)?);
    append_steps(
        id,
        lanes.postludes,
        ctx,
        needs_envs,
        step_context,
        rendered,
        "postlude",
    )
}

fn append_steps(
    id: &str,
    source: &BTreeMap<String, Vec<Step>>,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    step_context: &JobStepContext<'_>,
    rendered: &mut Vec<Yaml>,
    label: &str,
) -> Result<(), RenderError> {
    let steps = source
        .get(id)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("shared_lane_missing_{label}:{id}")))?;
    for step in steps {
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
    Ok(())
}

fn valid_shared_checkout(checkout: &Step, expected_uses: &str) -> bool {
    checkout.role == Some(StepRole::Checkout)
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        )
}
