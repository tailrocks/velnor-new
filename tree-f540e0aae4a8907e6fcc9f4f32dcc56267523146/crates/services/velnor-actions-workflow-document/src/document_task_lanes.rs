//! Static task factoring for report-producing Rust lane jobs.
//!
//! Paired hosted and Scale Set lanes keep their provider-owned setup and report
//! lifecycle in the workflow job. Their identical task body is shared at the
//! action path already assigned to that lane pair.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract_workflow::{Job, JobOutput, Step, StepRole};
use velnor_actions_workflow_jobs::RenderContext;
use velnor_actions_workflow_steps::RenderError;

use crate::document_lanes::references_any_step_output;
use crate::document_lanes::task_composite_file;
use crate::lane_share::{LaneShare, SharedActionCall, SharedActionInput};
use crate::lane_share_sections::task_factor_error;

pub(crate) fn factor_task_report_jobs(
    source: &BTreeMap<String, Job>,
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    factor_paired_report_jobs(source, shared, ctx)?;
    factor_unpaired_report_jobs(source, shared, ctx)
}

fn factor_unpaired_report_jobs(
    source: &BTreeMap<String, Job>,
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    for (id, job) in source {
        if is_unpaired_report_job(id, job, shared) {
            factor_unpaired_report_job(id, job, shared, ctx)?;
        }
    }
    Ok(())
}

fn is_unpaired_report_job(id: &str, job: &Job, shared: &LaneShare) -> bool {
    id.starts_with("rust-")
        && !shared.calls.contains_key(id)
        && job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::CrateReportUpload))
}

fn factor_unpaired_report_job(
    id: &str,
    job: &Job,
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    crate::lane_share_sections::validate_task_job_id(id)?;
    validate_unpaired_report_contract(id, job)?;
    let Some((checkout, remaining)) =
        crate::lane_share::peel_checkout(&job.steps, &ctx.checkout_uses)
    else {
        return Err(task_factor_error(id, "checkout_invalid"));
    };
    let Some((body, postlude)) = split_static_task_steps(remaining) else {
        return Err(task_factor_error(id, "outer_step_order_unsupported"));
    };
    let logical = format!("task-{id}");
    let (file, inputs) = task_composite_file(&logical, &body, &postlude, ctx)?;
    if shared.files.iter().any(|known| known.path == file.path) {
        return Err(task_factor_error(id, "path_collision"));
    }
    register_unpaired_report_action(id, job, checkout, postlude, file, inputs, shared)
}

fn validate_unpaired_report_contract(id: &str, job: &Job) -> Result<(), RenderError> {
    let upload_count = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::CrateReportUpload))
        .count();
    if upload_count != 1 || !job.outputs.contains(&JobOutput::task_report_artifact_id()) {
        return Err(task_factor_error(id, "report_output_mismatch"));
    }
    if !job.needs.iter().any(|need| need == "plan") {
        return Err(task_factor_error(id, "plan_dependency_missing"));
    }
    Ok(())
}

fn register_unpaired_report_action(
    id: &str,
    job: &Job,
    checkout: &Step,
    postlude: Vec<Step>,
    file: velnor_actions_workflow_tree::rendered::RenderedFile,
    inputs: Vec<SharedActionInput>,
    shared: &mut LaneShare,
) -> Result<(), RenderError> {
    let logical = format!("task-{id}");
    shared.calls.insert(
        id.to_owned(),
        SharedActionCall {
            uses: format!("./.github/actions/{logical}"),
            inputs,
        },
    );
    shared.checkouts.insert(id.to_owned(), checkout.clone());
    shared.env_steps.insert(id.to_owned(), job.steps.clone());
    shared.prefixes.insert(id.to_owned(), Vec::new());
    shared.preludes.insert(id.to_owned(), Vec::new());
    shared.postludes.insert(id.to_owned(), postlude.clone());
    let Some(job) = shared.jobs.get_mut(id) else {
        return Err(task_factor_error(id, "job_missing"));
    };
    job.steps = postlude;
    shared.files.push(file);
    Ok(())
}

fn factor_paired_report_jobs(
    source: &BTreeMap<String, Job>,
    shared: &mut LaneShare,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    for (hosted_id, hosted) in source {
        let Some(logical) = hosted_id.strip_suffix(HOSTED_SUFFIX) else {
            continue;
        };
        if !is_report_job(hosted_id, hosted) {
            continue;
        }
        let local_id = format!("{logical}{SCALE_SUFFIX}");
        let Some(local) = source.get(&local_id) else {
            continue;
        };
        if !is_report_job(&local_id, local) {
            return Err(task_factor_error(hosted_id, "paired_report_mismatch"));
        }
        let (Some(hosted_call), Some(local_call)) =
            (shared.calls.get(hosted_id), shared.calls.get(&local_id))
        else {
            return Err(task_factor_error(hosted_id, "paired_call_missing"));
        };
        let action_path = format!("./.github/actions/{logical}");
        if hosted_call.uses != action_path || local_call.uses != action_path {
            return Err(task_factor_error(hosted_id, "paired_action_mismatch"));
        }
        if !hosted_call.inputs.is_empty() || !local_call.inputs.is_empty() {
            return Err(task_factor_error(hosted_id, "paired_inputs_unexpected"));
        }
        validate_report_job(hosted_id, hosted)?;
        validate_report_job(&local_id, local)?;
        let (hosted_body, hosted_postlude) = original_task_parts(hosted_id, hosted, shared)?;
        let (local_body, local_postlude) = original_task_parts(&local_id, local, shared)?;
        if hosted_body != local_body {
            return Err(task_factor_error(hosted_id, "paired_task_body_differs"));
        }
        validate_postlude_scope(hosted_id, &hosted_body, &hosted_postlude)?;
        validate_postlude_scope(&local_id, &hosted_body, &local_postlude)?;

        let (file, inputs) = task_composite_file(logical, &hosted_body, &hosted_postlude, ctx)?;
        replace_existing_action(hosted_id, shared, file)?;
        install_covered_tasks_input(hosted_id, &local_id, shared, &inputs)?;
        retain_outer_steps(hosted_id, hosted_postlude, shared)?;
        retain_outer_steps(&local_id, local_postlude, shared)?;
    }
    Ok(())
}

fn is_report_job(id: &str, job: &Job) -> bool {
    id.starts_with("rust-")
        && job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::CrateReportUpload))
}

fn validate_report_job(id: &str, job: &Job) -> Result<(), RenderError> {
    let upload_count = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::CrateReportUpload))
        .count();
    let artifact_output = JobOutput::task_report_artifact_id();
    let artifact_output_count = job
        .outputs
        .iter()
        .filter(|output| output.name == artifact_output.name)
        .count();
    if upload_count != 1 || artifact_output_count != 1 || !job.outputs.contains(&artifact_output) {
        return Err(task_factor_error(id, "report_output_mismatch"));
    }
    if !job.needs.iter().any(|need| need == "plan") {
        return Err(task_factor_error(id, "plan_dependency_missing"));
    }
    Ok(())
}

fn original_task_parts(
    id: &str,
    job: &Job,
    shared: &LaneShare,
) -> Result<(Vec<Step>, Vec<Step>), RenderError> {
    let fail = || task_factor_error(id, "paired_step_context_mismatch");
    let checkout = shared.checkouts.get(id).ok_or_else(fail)?;
    let after_checkout = job
        .steps
        .strip_prefix(std::slice::from_ref(checkout))
        .ok_or_else(fail)?;
    let prefix = shared.prefixes.get(id).ok_or_else(fail)?;
    let after_prefix = after_checkout
        .strip_prefix(prefix.as_slice())
        .ok_or_else(fail)?;
    let prelude = shared.preludes.get(id).ok_or_else(fail)?;
    let body_and_postlude = after_prefix
        .strip_prefix(prelude.as_slice())
        .ok_or_else(fail)?;
    split_static_task_steps(body_and_postlude)
        .ok_or_else(|| task_factor_error(id, "outer_step_order_unsupported"))
}

fn validate_postlude_scope(id: &str, body: &[Step], postlude: &[Step]) -> Result<(), RenderError> {
    let ids = body
        .iter()
        .filter_map(|step| step.id.map(|step_id| step_id.as_str().to_owned()))
        .collect();
    if postlude
        .iter()
        .any(|step| references_any_step_output(step, &ids))
    {
        return Err(task_factor_error(id, "postlude_output_scope"));
    }
    Ok(())
}

fn replace_existing_action(
    id: &str,
    shared: &mut LaneShare,
    file: velnor_actions_workflow_tree::rendered::RenderedFile,
) -> Result<(), RenderError> {
    let matches: Vec<_> = shared
        .files
        .iter()
        .enumerate()
        .filter(|(_, existing)| existing.path == file.path)
        .map(|(index, _)| index)
        .collect();
    if matches.len() != 1 {
        return Err(task_factor_error(id, "existing_action_count"));
    }
    shared.files[matches[0]] = file;
    Ok(())
}

fn install_covered_tasks_input(
    hosted_id: &str,
    local_id: &str,
    shared: &mut LaneShare,
    inputs: &[SharedActionInput],
) -> Result<(), RenderError> {
    for id in [hosted_id, local_id] {
        let call = shared
            .calls
            .get_mut(id)
            .ok_or_else(|| task_factor_error(id, "paired_call_missing"))?;
        call.inputs.clear();
        call.inputs.extend(inputs.iter().copied());
    }
    Ok(())
}

fn retain_outer_steps(
    id: &str,
    postlude: Vec<Step>,
    shared: &mut LaneShare,
) -> Result<(), RenderError> {
    let Some(job) = shared.jobs.get_mut(id) else {
        return Err(task_factor_error(id, "job_missing"));
    };
    job.steps.clone_from(&postlude);
    shared.postludes.insert(id.to_owned(), postlude);
    Ok(())
}

pub(crate) fn split_static_task_steps(steps: &[Step]) -> Option<(Vec<Step>, Vec<Step>)> {
    let outer_at = steps
        .iter()
        .position(is_outer_task_step)
        .unwrap_or(steps.len());
    let (body, postlude) = steps.split_at(outer_at);
    if body.is_empty() || postlude.iter().any(|step| !is_outer_task_step(step)) {
        return None;
    }
    if postlude
        .iter()
        .filter(|step| step.role == Some(StepRole::CrateReportUpload))
        .count()
        != 1
        || body.iter().any(|step| {
            matches!(
                step.role,
                Some(StepRole::TofuProvidersRestore | StepRole::TofuProviderUse)
            )
        })
    {
        return None;
    }
    let inner_ids = body
        .iter()
        .filter_map(|step| step.id.map(|id| id.as_str().to_owned()))
        .collect();
    if postlude
        .iter()
        .any(|step| references_any_step_output(step, &inner_ids))
    {
        return None;
    }
    Some((body.to_vec(), postlude.to_vec()))
}

fn is_outer_task_step(step: &Step) -> bool {
    has_non_success_status_condition(step)
        || matches!(
            step.role,
            Some(
                StepRole::CrateReportUpload
                    | StepRole::MatrixReportUpload
                    | StepRole::ToolsCacheSave
                    | StepRole::CargoSourcesSave
                    | StepRole::TofuProvidersSave
            )
        )
}

fn has_non_success_status_condition(step: &Step) -> bool {
    step.condition.as_deref().is_some_and(|condition| {
        ["always", "failure", "cancelled"]
            .into_iter()
            .any(|function| contains_function_call(condition, function))
    })
}

fn contains_function_call(value: &str, function: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = value[offset..].find(function) {
        let start = offset + relative;
        let end = start + function.len();
        let left_boundary = start == 0 || !is_expression_name_byte(value.as_bytes()[start - 1]);
        let mut after = end;
        while value
            .as_bytes()
            .get(after)
            .is_some_and(u8::is_ascii_whitespace)
        {
            after += 1;
        }
        if left_boundary && value.as_bytes().get(after) == Some(&b'(') {
            return true;
        }
        offset = end;
        if offset >= value.len() {
            return false;
        }
    }
    false
}

fn is_expression_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
