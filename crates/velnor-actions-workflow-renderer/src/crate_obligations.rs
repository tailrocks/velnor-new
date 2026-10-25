use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::action_ref::CRATE_OBLIGATION_ACTION_PREFIX;
use crate::lane_share::LaneShare;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, composite, marker, steps, yaml};

pub(crate) fn extract_obligation_composites(
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<usize, RenderError> {
    let ids: Vec<String> = shared
        .jobs
        .keys()
        .filter(|id| !shared.calls.contains_key(*id))
        .cloned()
        .collect();
    let mut files = Vec::new();
    for id in ids {
        let Some(job) = shared.jobs.get_mut(&id) else {
            continue;
        };
        if let Some(file) = extract_job(&id, job, ctx)? {
            if shared
                .files
                .iter()
                .any(|existing| existing.path == file.path)
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "crate_action_collision:{}",
                    file.path
                )));
            }
            files.push(file);
        }
    }
    let count = files.len();
    shared.files.extend(files);
    Ok(count)
}

fn extract_job(
    job_id: &str,
    job: &mut Job,
    ctx: &RenderContext,
) -> Result<Option<RenderedFile>, RenderError> {
    if job.steps.iter().any(|step| step.parallel) {
        return Ok(None);
    }
    let Some(start) = job.steps.iter().position(is_obligation_step) else {
        return Ok(None);
    };
    let end = job
        .steps
        .iter()
        .rposition(is_obligation_step)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("crate_obligation_missing:{job_id}"))
        })?;
    if job.steps[start..=end]
        .iter()
        .any(|step| !is_obligation_step(step))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "crate_obligations_split:{job_id}"
        )));
    }
    let obligations = job.steps[start..=end].to_vec();
    let file = obligation_action(job_id, &obligations, ctx)?;
    let call = steps::action_step(
        "Run crate obligations",
        &format!("{CRATE_OBLIGATION_ACTION_PREFIX}{job_id}"),
        BTreeMap::new(),
    )?;
    let mut steps = Vec::with_capacity(job.steps.len() - obligations.len() + 1);
    steps.extend(job.steps[..start].iter().cloned());
    steps.push(call);
    steps.extend(job.steps[end + 1..].iter().cloned());
    job.steps = steps;
    Ok(Some(file))
}

fn is_obligation_step(step: &Step) -> bool {
    matches!(step.kind, StepKind::TaskExecution { .. })
        || step.role == Some(StepRole::TofuProviderUse)
}

fn obligation_action(
    job_id: &str,
    obligations: &[Step],
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(
        obligations,
        &format!("composite:{job_id}"),
    )
    .map_err(RenderError::Contract)?;
    let mut rendered = Vec::with_capacity(obligations.len());
    let empty_job_env = BTreeMap::new();
    let step_context = crate::document_lanes::JobStepContext {
        job_env: &empty_job_env,
        runs_on: None,
        actions_read: false,
    };
    for step in obligations {
        rendered.push(crate::document_steps::step_to_yaml(
            job_id,
            step,
            ctx,
            &[],
            true,
            &step_context,
        )?);
    }
    let body = composite::composite_yaml(job_id, rendered)?;
    let bytes = marker::with_marker(&ctx.generator_version, &yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/crate-obligation-{job_id}/action.yml"),
        bytes,
    })
}
