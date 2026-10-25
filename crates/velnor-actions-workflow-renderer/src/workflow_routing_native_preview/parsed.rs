use saphyr::{Mapping, Yaml};

use crate::{RenderError, workflow_routing::invalid};

use super::WorkflowRoutingPolicy;
use super::{
    ASSEMBLE_JOB, ASSEMBLE_RUNNER, PAYLOAD_JOB, PAYLOAD_MATRIX, PAYLOAD_RUNNER, PREVIEW_PATH,
    SOURCE_JOB, SOURCE_OPERATION, SOURCE_STEP,
};

pub(super) fn validate_workflow(
    path: &str,
    workflow: &Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    if path != PREVIEW_PATH {
        return Err(invalid(format!("native_preview_path_invalid:{path}")));
    }
    let document =
        mapping(workflow).ok_or_else(|| invalid(format!("workflow_document_not_map:{path}")))?;
    let jobs = mapping(required(document, "jobs", path, "workflow")?)
        .ok_or_else(|| invalid(format!("workflow_jobs_missing:{path}")))?;
    let source = mapping(required(jobs, SOURCE_JOB, path, "jobs")?)
        .ok_or_else(|| invalid(format!("native_preview_source_job_invalid:{path}")))?;
    validate_source(source, path, policy)?;
    let payload = required(jobs, PAYLOAD_JOB, path, "jobs")?;
    validate_payload(payload, path, policy)?;
    let assemble = required(jobs, ASSEMBLE_JOB, path, "jobs")?;
    validate_assemble(assemble, path, policy)
}

fn validate_source(
    fields: &Mapping<'_>,
    path: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let outputs = mapping(required(fields, "outputs", path, SOURCE_JOB)?)
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
        if text(required(
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
    let steps = sequence(required(fields, "steps", path, SOURCE_JOB)?)
        .ok_or_else(|| invalid(format!("native_preview_source_steps_invalid:{path}")))?;
    let mut source = None;
    for step in steps {
        let step = mapping(step)
            .ok_or_else(|| invalid(format!("native_preview_source_step_invalid:{path}")))?;
        let Some(id) = optional(step, "id", path, "preview-eligibility.step")? else {
            continue;
        };
        if text(id) == Some(SOURCE_STEP) && source.replace(step).is_some() {
            return Err(invalid(format!(
                "native_preview_source_step_duplicate:{path}"
            )));
        }
    }
    let source =
        source.ok_or_else(|| invalid(format!("native_preview_source_step_missing:{path}")))?;
    let env = mapping(required(source, "env", path, SOURCE_STEP)?)
        .ok_or_else(|| invalid(format!("native_preview_source_env_invalid:{path}")))?;
    if text(required(
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
    let scale_set_requirement =
        optional(env, "VELNOR_RELEASE_REQUIRE_LINUX_AMD64", path, SOURCE_STEP)?;
    if policy.is_scale_set() {
        if scale_set_requirement.and_then(text) != Some("true") {
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

fn validate_payload(
    value: &Yaml<'_>,
    path: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let fields = mapping(value)
        .ok_or_else(|| invalid(format!("native_preview_payload_job_invalid:{path}")))?;
    if !runner_matches(
        required(fields, "runs-on", path, PAYLOAD_JOB)?,
        PAYLOAD_RUNNER,
        policy,
    ) {
        return Err(invalid(format!(
            "native_preview_runner_contract_invalid:{path}:{PAYLOAD_JOB}"
        )));
    }
    validate_needs(fields, &[SOURCE_JOB], path, PAYLOAD_JOB)?;
    let strategy = mapping(required(fields, "strategy", path, PAYLOAD_JOB)?)
        .ok_or_else(|| invalid(format!("native_preview_matrix_invalid:{path}")))?;
    if strategy.len() != 2
        || required(strategy, "fail-fast", path, "preview-payload.strategy")?.as_bool()
            != Some(false)
        || text(required(
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

fn validate_assemble(
    value: &Yaml<'_>,
    path: &str,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let fields = mapping(value)
        .ok_or_else(|| invalid(format!("native_preview_assemble_job_invalid:{path}")))?;
    if !runner_matches(
        required(fields, "runs-on", path, ASSEMBLE_JOB)?,
        ASSEMBLE_RUNNER,
        policy,
    ) {
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

fn runner_matches(value: &Yaml<'_>, dynamic: &str, policy: &WorkflowRoutingPolicy) -> bool {
    if !policy.is_scale_set() {
        return text(value) == Some(dynamic);
    }
    let Some(selector) = policy.selector() else {
        return false;
    };
    let Some(labels) = sequence(value) else {
        return false;
    };
    labels.len() == selector.labels().len()
        && labels
            .iter()
            .zip(selector.labels())
            .all(|(actual, expected)| text(actual) == Some(expected))
}

fn validate_needs(
    fields: &Mapping<'_>,
    expected: &[&str],
    path: &str,
    owner: &str,
) -> Result<(), RenderError> {
    let actual = sequence(required(fields, "needs", path, owner)?)
        .ok_or_else(|| invalid(format!("native_preview_needs_invalid:{path}:{owner}")))?;
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected)
            .any(|(actual, expected)| text(actual) != Some(*expected))
    {
        return Err(invalid(format!(
            "native_preview_needs_invalid:{path}:{owner}"
        )));
    }
    Ok(())
}

fn required<'a, 'input>(
    fields: &'a Mapping<'input>,
    key: &str,
    path: &str,
    owner: &str,
) -> Result<&'a Yaml<'input>, RenderError> {
    let mut entries = fields.iter().filter(|(name, _)| text(name) == Some(key));
    let value = entries
        .next()
        .map(|(_, value)| value)
        .ok_or_else(|| invalid(format!("native_preview_field_missing:{path}:{owner}:{key}")))?;
    if entries.next().is_some() {
        return Err(invalid(format!(
            "native_preview_field_duplicate:{path}:{owner}:{key}"
        )));
    }
    Ok(value)
}

fn optional<'a, 'input>(
    fields: &'a Mapping<'input>,
    key: &str,
    path: &str,
    owner: &str,
) -> Result<Option<&'a Yaml<'input>>, RenderError> {
    let mut entries = fields.iter().filter(|(name, _)| text(name) == Some(key));
    let value = entries.next().map(|(_, value)| value);
    if entries.next().is_some() {
        return Err(invalid(format!(
            "native_preview_field_duplicate:{path}:{owner}:{key}"
        )));
    }
    Ok(value)
}

fn mapping<'a, 'input>(value: &'a Yaml<'input>) -> Option<&'a saphyr::Mapping<'input>> {
    untagged(value).as_mapping()
}

fn sequence<'a, 'input>(value: &'a Yaml<'input>) -> Option<&'a [Yaml<'input>]> {
    match untagged(value) {
        Yaml::Sequence(values) => Some(values),
        _ => None,
    }
}

fn text<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    untagged(value).as_str()
}

fn untagged<'a, 'input>(value: &'a Yaml<'input>) -> &'a Yaml<'input> {
    match value {
        Yaml::Tagged(_, inner) => untagged(inner),
        _ => value,
    }
}
