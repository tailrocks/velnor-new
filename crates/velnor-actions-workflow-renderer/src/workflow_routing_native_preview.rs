use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::ExecutionMode;

use super::{WorkflowRoutingPolicy, invalid};

#[path = "workflow_routing_native_preview/parsed.rs"]
mod parsed;

const PREVIEW_PATH: &str = ".github/workflows/binary-preview.yml";
const SOURCE_JOB: &str = "preview-eligibility";
const SOURCE_STEP: &str = "source";
const SOURCE_OPERATION: &str = "finalize-preview-source-v1";
const PAYLOAD_JOB: &str = "preview-payload";
const ASSEMBLE_JOB: &str = "preview-assemble";
const PAYLOAD_RUNNER: &str = "${{ matrix.runner }}";
const PAYLOAD_MATRIX: &str = "${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}";
const ASSEMBLE_RUNNER: &str = "${{ needs.preview-eligibility.outputs.producer_runner }}";

#[derive(Clone, Copy)]
pub(super) struct Context;

pub(super) const fn preview_path() -> &'static str {
    PREVIEW_PATH
}

pub(super) fn validate_emitted_workflow(
    path: &str,
    workflow: &Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    if path != PREVIEW_PATH {
        return Err(invalid(format!("native_preview_path_invalid:{path}")));
    }
    let document =
        entries(workflow).ok_or_else(|| invalid(format!("workflow_document_not_map:{path}")))?;
    let jobs = entries(required(document, "jobs", path, "workflow")?)
        .ok_or_else(|| invalid(format!("workflow_jobs_missing:{path}")))?;
    let source = required(jobs, SOURCE_JOB, path, "jobs")?;
    let source = entries(source)
        .ok_or_else(|| invalid(format!("native_preview_source_job_invalid:{path}")))?;
    validate_source_outputs(source, path)?;
    validate_source_step(source, path, policy)?;
    validate_payload_job(required(jobs, PAYLOAD_JOB, path, "jobs")?, path)?;
    validate_assemble_job(required(jobs, ASSEMBLE_JOB, path, "jobs")?, path)
}

pub(super) fn route_runner(
    _context: Context,
    fields: &mut [(String, Yaml)],
    runs_on_index: usize,
    path: &str,
    job_id: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<Option<bool>, RenderError> {
    let expected = match (path, job_id) {
        (PREVIEW_PATH, PAYLOAD_JOB) => PAYLOAD_RUNNER,
        (PREVIEW_PATH, ASSEMBLE_JOB) => ASSEMBLE_RUNNER,
        _ => return Ok(None),
    };
    if scalar(&fields[runs_on_index].1) != Some(expected) {
        return Err(invalid(format!(
            "native_preview_runner_contract_invalid:{path}:{job_id}"
        )));
    }
    if fields.iter().any(|(key, _)| key == "container") {
        return Err(invalid(format!(
            "native_preview_container_unexpected:{path}:{job_id}"
        )));
    }
    if policy.mode == ExecutionMode::ScaleSet {
        let selector = policy
            .selector()
            .ok_or_else(|| invalid(format!("scale_set_selector_unconfigured:{path}:{job_id}")))?;
        fields[runs_on_index].1 = Yaml::Flow(selector.labels().to_vec());
        Ok(Some(true))
    } else {
        Ok(Some(false))
    }
}

pub(super) fn accept_final_runner(
    _context: Context,
    path: &str,
    job_id: &str,
    runs_on: Option<&str>,
    has_container: bool,
    policy: &WorkflowRoutingPolicy,
) -> Result<bool, RenderError> {
    let expected = match (path, job_id) {
        (PREVIEW_PATH, PAYLOAD_JOB) => PAYLOAD_RUNNER,
        (PREVIEW_PATH, ASSEMBLE_JOB) => ASSEMBLE_RUNNER,
        _ => return Ok(false),
    };
    if policy.mode == ExecutionMode::ScaleSet {
        if runs_on == Some(expected) {
            return Err(invalid(format!(
                "native_preview_runner_not_routed:{path}:{job_id}"
            )));
        }
        return Ok(false);
    }
    if runs_on != Some(expected) || has_container {
        return Err(invalid(format!(
            "native_preview_runner_contract_invalid:{path}:{job_id}"
        )));
    }
    Ok(true)
}

pub(super) fn validate_parsed_workflow(
    _context: Context,
    path: &str,
    workflow: &saphyr::Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    parsed::validate_workflow(path, workflow, policy)
}

fn validate_source_outputs(fields: &[(String, Yaml)], path: &str) -> Result<(), RenderError> {
    let outputs = entries(required(fields, "outputs", path, SOURCE_JOB)?)
        .ok_or_else(|| invalid(format!("native_preview_source_outputs_invalid:{path}")))?;
    for (name, expected) in [
        (
            "payload_matrix",
            "${{ steps.source.outputs.payload_matrix }}",
        ),
        (
            "producer_runner",
            "${{ steps.source.outputs.producer_runner }}",
        ),
    ] {
        if scalar(required(
            outputs,
            name,
            path,
            "preview-eligibility.outputs",
        )?) != Some(expected)
        {
            return Err(invalid(format!(
                "native_preview_source_output_invalid:{path}:{name}"
            )));
        }
    }
    Ok(())
}

fn validate_source_step(
    fields: &[(String, Yaml)],
    path: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let steps = sequence(required(fields, "steps", path, SOURCE_JOB)?)
        .ok_or_else(|| invalid(format!("native_preview_source_steps_invalid:{path}")))?;
    let mut source = None;
    for step in steps {
        let fields = entries(step)
            .ok_or_else(|| invalid(format!("native_preview_source_step_invalid:{path}")))?;
        let Some(id) = optional(fields, "id", path, "preview-eligibility.step")? else {
            continue;
        };
        if scalar(id) == Some(SOURCE_STEP) && source.replace(fields).is_some() {
            return Err(invalid(format!(
                "native_preview_source_step_duplicate:{path}"
            )));
        }
    }
    let source =
        source.ok_or_else(|| invalid(format!("native_preview_source_step_missing:{path}")))?;
    let env = entries(required(source, "env", path, SOURCE_STEP)?)
        .ok_or_else(|| invalid(format!("native_preview_source_env_invalid:{path}")))?;
    if scalar(required(
        env,
        "VELNOR_INTERNAL_OP",
        path,
        "preview-source.env",
    )?) != Some(SOURCE_OPERATION)
    {
        return Err(invalid(format!(
            "native_preview_source_operation_invalid:{path}"
        )));
    }
    let scale_set_requirement = optional(
        env,
        "VELNOR_RELEASE_REQUIRE_LINUX_AMD64",
        path,
        "preview-source.env",
    )?;
    if policy.is_scale_set() {
        if scale_set_requirement.and_then(scalar) != Some("true") {
            return Err(invalid(format!(
                "native_preview_scale_set_admission_missing:{path}"
            )));
        }
    } else if scale_set_requirement.is_some() {
        return Err(invalid(format!(
            "native_preview_scale_set_admission_unexpected:{path}"
        )));
    }
    Ok(())
}

fn validate_payload_job(value: &Yaml, path: &str) -> Result<(), RenderError> {
    let fields = entries(value)
        .ok_or_else(|| invalid(format!("native_preview_payload_job_invalid:{path}")))?;
    if scalar(required(fields, "runs-on", path, PAYLOAD_JOB)?) != Some(PAYLOAD_RUNNER) {
        return Err(invalid(format!(
            "native_preview_runner_contract_invalid:{path}:{PAYLOAD_JOB}"
        )));
    }
    validate_needs(fields, &[SOURCE_JOB], path, PAYLOAD_JOB)?;
    let strategy = entries(required(fields, "strategy", path, PAYLOAD_JOB)?)
        .ok_or_else(|| invalid(format!("native_preview_matrix_invalid:{path}")))?;
    if strategy.len() != 2
        || !matches!(
            required(strategy, "fail-fast", path, "preview-payload.strategy")?,
            Yaml::Bool(false)
        )
        || scalar(required(
            strategy,
            "matrix",
            path,
            "preview-payload.strategy",
        )?) != Some(PAYLOAD_MATRIX)
    {
        return Err(invalid(format!("native_preview_matrix_invalid:{path}")));
    }
    Ok(())
}

fn validate_assemble_job(value: &Yaml, path: &str) -> Result<(), RenderError> {
    let fields = entries(value)
        .ok_or_else(|| invalid(format!("native_preview_assemble_job_invalid:{path}")))?;
    if scalar(required(fields, "runs-on", path, ASSEMBLE_JOB)?) != Some(ASSEMBLE_RUNNER) {
        return Err(invalid(format!(
            "native_preview_runner_contract_invalid:{path}:{ASSEMBLE_JOB}"
        )));
    }
    validate_needs(fields, &[SOURCE_JOB, PAYLOAD_JOB], path, ASSEMBLE_JOB)?;
    if optional(fields, "strategy", path, ASSEMBLE_JOB)?.is_some() {
        return Err(invalid(format!(
            "native_preview_assemble_strategy_unexpected:{path}"
        )));
    }
    Ok(())
}

fn validate_needs(
    fields: &[(String, Yaml)],
    expected: &[&str],
    path: &str,
    job: &str,
) -> Result<(), RenderError> {
    let actual = sequence(required(fields, "needs", path, job)?)
        .ok_or_else(|| invalid(format!("native_preview_needs_invalid:{path}:{job}")))?;
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected)
            .any(|(actual, expected)| scalar(actual) != Some(expected))
    {
        return Err(invalid(format!(
            "native_preview_needs_invalid:{path}:{job}"
        )));
    }
    Ok(())
}

fn entries(value: &Yaml) -> Option<&[(String, Yaml)]> {
    match value {
        Yaml::Map(fields)
        | Yaml::AnchoredMap {
            entries: fields, ..
        } => Some(fields),
        _ => None,
    }
}

fn sequence(value: &Yaml) -> Option<&[Yaml]> {
    match value {
        Yaml::Seq(values) => Some(values),
        _ => None,
    }
}

fn required<'a>(
    fields: &'a [(String, Yaml)],
    key: &str,
    path: &str,
    owner: &str,
) -> Result<&'a Yaml, RenderError> {
    let mut values = fields.iter().filter(|(name, _)| name == key);
    let value = values
        .next()
        .map(|(_, value)| value)
        .ok_or_else(|| invalid(format!("native_preview_field_missing:{path}:{owner}:{key}")))?;
    if values.next().is_some() {
        return Err(invalid(format!(
            "native_preview_field_duplicate:{path}:{owner}:{key}"
        )));
    }
    Ok(value)
}

fn optional<'a>(
    fields: &'a [(String, Yaml)],
    key: &str,
    path: &str,
    owner: &str,
) -> Result<Option<&'a Yaml>, RenderError> {
    let mut values = fields.iter().filter(|(name, _)| name == key);
    let value = values.next().map(|(_, value)| value);
    if values.next().is_some() {
        return Err(invalid(format!(
            "native_preview_field_duplicate:{path}:{owner}:{key}"
        )));
    }
    Ok(value)
}

fn scalar(value: &Yaml) -> Option<&str> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Some(value),
        _ => None,
    }
}

#[cfg(test)]
#[path = "workflow_routing_native_preview_tests.rs"]
mod tests;
