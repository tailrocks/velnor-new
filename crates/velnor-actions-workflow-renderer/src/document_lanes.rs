use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::composite::shared_call;
use crate::document_steps::{StepRenderContext, step_to_yaml};
use crate::render::RenderContext;
use crate::yaml::Yaml;

pub(crate) struct SharedLaneSteps<'a> {
    pub checkouts: &'a BTreeMap<String, Step>,
    pub env_steps: &'a BTreeMap<String, Vec<Step>>,
    pub runtime_preludes: &'a BTreeMap<String, Vec<Step>>,
    pub prefixes: &'a BTreeMap<String, Vec<Step>>,
    pub preludes: &'a BTreeMap<String, Vec<Step>>,
    pub postludes: &'a BTreeMap<String, Vec<Step>>,
}

pub(crate) struct JobStepContext<'a> {
    pub job_env: &'a BTreeMap<String, String>,
    pub actions_read: bool,
}

struct LaneJobContext<'a> {
    id: &'a str,
    runs_on: &'a str,
    ctx: &'a RenderContext,
    needs_envs: &'a [(String, String)],
    step_context: &'a JobStepContext<'a>,
}

impl LaneJobContext<'_> {
    fn step_render_context(&self, composite: bool) -> StepRenderContext<'_> {
        StepRenderContext {
            job_id: self.id,
            ctx: self.ctx,
            needs_envs: self.needs_envs,
            composite,
            job_env: self.step_context.job_env,
            actions_read: self.step_context.actions_read,
            runs_on: Some(self.runs_on),
        }
    }
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
    let lane_job = LaneJobContext {
        id,
        runs_on: &job.runs_on,
        ctx,
        needs_envs,
        step_context,
    };
    if let Some(uses) = shared {
        append_shared_lane_steps(&lane_job, uses, lanes, &mut rendered)?;
    } else {
        if lanes.checkouts.contains_key(id) {
            return Err(RenderError::InvalidWorkflow(format!(
                "checkout_without_shared_lane:{id}"
            )));
        }
        for step in &job.steps {
            rendered.push(step_to_yaml(step, &lane_job.step_render_context(false))?);
        }
    }
    Ok(rendered)
}

fn append_shared_lane_steps(
    job: &LaneJobContext<'_>,
    uses: &str,
    lanes: &SharedLaneSteps<'_>,
    rendered: &mut Vec<Yaml>,
) -> Result<(), RenderError> {
    let Some(checkout) = lanes.checkouts.get(job.id) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "shared_lane_missing_checkout:{}",
            job.id
        )));
    };
    if !valid_shared_checkout(checkout, &job.ctx.checkout_uses) {
        return Err(RenderError::InvalidWorkflow(format!(
            "shared_lane_invalid_checkout:{}",
            job.id
        )));
    }
    append_one(job, checkout, rendered)?;
    append_steps(job, lanes.runtime_preludes, rendered, "runtime_prelude")?;
    append_steps(job, lanes.prefixes, rendered, "prefix")?;
    append_steps(job, lanes.preludes, rendered, "prelude")?;
    rendered.push(shared_call(uses)?);
    append_steps(job, lanes.postludes, rendered, "postlude")
}

fn append_steps(
    job: &LaneJobContext<'_>,
    source: &BTreeMap<String, Vec<Step>>,
    rendered: &mut Vec<Yaml>,
    label: &str,
) -> Result<(), RenderError> {
    let steps = source.get(job.id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("shared_lane_missing_{label}:{}", job.id))
    })?;
    for step in steps {
        append_one(job, step, rendered)?;
    }
    Ok(())
}

fn append_one(
    job: &LaneJobContext<'_>,
    step: &Step,
    rendered: &mut Vec<Yaml>,
) -> Result<(), RenderError> {
    rendered.push(step_to_yaml(step, &job.step_render_context(false))?);
    Ok(())
}

fn valid_shared_checkout(checkout: &Step, expected_uses: &str) -> bool {
    checkout.name == "Checkout"
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        )
}
