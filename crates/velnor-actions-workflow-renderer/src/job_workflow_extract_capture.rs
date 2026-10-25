//! Explicit test-feature lowering and rendered-child context checks.

use std::collections::BTreeSet;

use velnor_actions_contract::Job;
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};

use super::{
    PreparedJob, RenderError, invalid,
    runner::{CaptureRunnerMetadata, capture_runner_metadata},
    selection::JobWorkflowSelection,
};

pub(crate) fn prepare_job_for_capture(
    source_id: &str,
    source: &Job,
    allowlisted_ids: &BTreeSet<String>,
) -> Result<PreparedJob, RenderError> {
    validate_capture_source_ids(allowlisted_ids)?;
    if !allowlisted_ids.contains(source_id) {
        return Err(invalid(source_id, "source_id_not_in_typed_rust_allowlist"));
    }
    capture_runner_metadata(source_id, source)?;
    super::prepare_job_inner(
        source_id,
        source,
        JobWorkflowSelection::explicit(allowlisted_ids),
    )
}

#[cfg(test)]
pub(crate) fn validate_finalized_runner(
    source_id: &str,
    source: &Job,
    parent: &crate::yaml::Yaml,
    expected_caller: &crate::yaml::Yaml,
) -> Result<CaptureRunnerMetadata, RenderError> {
    validate_finalized_source(source_id, source, parent, expected_caller, false)
}

pub(crate) fn validate_finalized_caller(
    source_id: &str,
    source: &Job,
    parent: &crate::yaml::Yaml,
    expected_caller: &crate::yaml::Yaml,
) -> Result<CaptureRunnerMetadata, RenderError> {
    validate_finalized_source(source_id, source, parent, expected_caller, true)
}

fn validate_finalized_source(
    source_id: &str,
    source: &Job,
    parent: &crate::yaml::Yaml,
    expected_caller: &crate::yaml::Yaml,
    require_caller: bool,
) -> Result<CaptureRunnerMetadata, RenderError> {
    let metadata = capture_runner_metadata(source_id, source)?;
    let fields = parent_job_fields(parent, source_id)?;
    if field(fields, "strategy").is_some() {
        return Err(invalid(source_id, "capture_runner_has_matrix_or_call"));
    }
    if field(fields, "uses").is_some() {
        let actual_caller = crate::yaml::Yaml::Map(fields.to_vec());
        if &actual_caller != expected_caller {
            return Err(invalid(
                source_id,
                "capture_runner_has_unexpected_workflow_call",
            ));
        }
        return Ok(metadata);
    }
    if require_caller {
        return Err(invalid(source_id, "capture_final_workflow_call_missing"));
    }
    let runs_on = field(fields, "runs-on")
        .ok_or_else(|| invalid(source_id, "capture_final_runs_on_missing"))?;
    let matches_final = match metadata.runner_class {
        "hosted-linux-x64" => {
            scalar(runs_on).is_some_and(|label| metadata.final_runner_labels == [label.to_owned()])
        }
        "configured-scale-set-linux-x64" => {
            matches!(runs_on, crate::yaml::Yaml::Flow(labels) if labels == &metadata.final_runner_labels)
        }
        _ => false,
    };
    if !matches_final {
        return Err(invalid(source_id, "capture_final_runner_metadata_mismatch"));
    }
    Ok(metadata)
}

pub(crate) fn validate_rendered_child_runner(
    file: &crate::tree::RenderedFile,
    source_id: &str,
    expected_labels: &[String],
) -> Result<(), RenderError> {
    let document = crate::workflow_routing::parse_authority_document(file)?;
    let jobs = parsed_field(&document, "jobs")
        .ok_or_else(|| invalid(source_id, "capture_child_jobs_missing"))?;
    let jobs = jobs
        .as_mapping()
        .ok_or_else(|| invalid(source_id, "capture_child_jobs_not_map"))?;
    if jobs.len() != 1 {
        return Err(invalid(source_id, "capture_child_job_count_mismatch"));
    }
    let Some((job_id, child)) = jobs.iter().next() else {
        return Err(invalid(source_id, "capture_child_job_missing"));
    };
    if job_id.as_str() != Some(source_id) {
        return Err(invalid(source_id, "capture_child_job_id_mismatch"));
    }
    if child.as_mapping().is_none() {
        return Err(invalid(source_id, "capture_child_job_not_map"));
    }
    if parsed_field(child, "strategy").is_some() || parsed_field(child, "uses").is_some() {
        return Err(invalid(
            source_id,
            "capture_child_runner_has_matrix_or_call",
        ));
    }
    let runs_on = parsed_field(child, "runs-on")
        .ok_or_else(|| invalid(source_id, "capture_child_runs_on_missing"))?;
    let actual_labels = if let Some(label) = runs_on.as_str() {
        vec![label.to_owned()]
    } else if let Some(labels) = runs_on.as_sequence() {
        labels
            .iter()
            .map(|label| label.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid(source_id, "capture_child_runner_label_not_scalar"))?
    } else {
        return Err(invalid(source_id, "capture_child_runs_on_not_static"));
    };
    if actual_labels.as_slice() != expected_labels {
        return Err(invalid(source_id, "capture_child_runner_metadata_mismatch"));
    }
    Ok(())
}

fn parent_job_fields<'a>(
    parent: &'a crate::yaml::Yaml,
    source_id: &str,
) -> Result<&'a [(String, crate::yaml::Yaml)], RenderError> {
    let crate::yaml::Yaml::Map(fields) = parent else {
        return Err(invalid(source_id, "capture_parent_not_map"));
    };
    let jobs =
        field(fields, "jobs").ok_or_else(|| invalid(source_id, "capture_parent_jobs_missing"))?;
    let crate::yaml::Yaml::Map(jobs) = jobs else {
        return Err(invalid(source_id, "capture_parent_jobs_not_map"));
    };
    let job = jobs
        .iter()
        .find(|(job_id, _)| job_id == source_id)
        .map(|(_, job)| job)
        .ok_or_else(|| invalid(source_id, "capture_final_job_missing"))?;
    let crate::yaml::Yaml::Map(fields) = job else {
        return Err(invalid(source_id, "capture_final_job_not_map"));
    };
    Ok(fields)
}

fn field<'a>(
    fields: &'a [(String, crate::yaml::Yaml)],
    name: &str,
) -> Option<&'a crate::yaml::Yaml> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn scalar(value: &crate::yaml::Yaml) -> Option<&str> {
    match value {
        crate::yaml::Yaml::Str(value)
        | crate::yaml::Yaml::Quoted(value)
        | crate::yaml::Yaml::Annotated { value, .. } => Some(value),
        _ => None,
    }
}

fn parsed_field<'node, 'input>(
    value: &'node saphyr::Yaml<'input>,
    name: &str,
) -> Option<&'node saphyr::Yaml<'input>> {
    value
        .as_mapping()?
        .iter()
        .find(|(key, _)| key.as_str() == Some(name))
        .map(|(_, value)| value)
}

pub(crate) fn validate_capture_source_ids(
    source_ids: &BTreeSet<String>,
) -> Result<(), RenderError> {
    if source_ids.is_empty() || !source_ids.len().is_multiple_of(2) {
        return Err(invalid("capture", "incomplete_lane_pair_inventory"));
    }
    for source_id in source_ids {
        let paired_id = if let Some(group_id) = source_id.strip_suffix(HOSTED_SUFFIX) {
            format!("{group_id}{SCALE_SUFFIX}")
        } else if let Some(group_id) = source_id.strip_suffix(SCALE_SUFFIX) {
            format!("{group_id}{HOSTED_SUFFIX}")
        } else {
            return Err(invalid(source_id, "source_id_has_no_typed_lane_suffix"));
        };
        if !source_ids.contains(&paired_id) {
            return Err(invalid(source_id, "source_id_missing_typed_lane_pair"));
        }
    }
    Ok(())
}

pub(crate) fn rendered_child_has_unbound_needs(
    file: &crate::tree::RenderedFile,
) -> Result<bool, RenderError> {
    let parsed = crate::workflow_routing::parse_authority_document(file)?;
    yaml_has_unbound_needs(&parsed)
}

fn yaml_has_unbound_needs(value: &saphyr::Yaml<'_>) -> Result<bool, RenderError> {
    if let Some(mapping) = value.as_mapping() {
        for (key, child) in mapping {
            let contains_unbound = if key.as_str() == Some("if") {
                match child.as_str() {
                    Some(text) => super::test_condition_has_unbound_needs(text)?,
                    None => false,
                }
            } else {
                yaml_has_unbound_needs(child)?
            };
            if contains_unbound {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if let Some(sequence) = value.as_sequence() {
        for child in sequence {
            if yaml_has_unbound_needs(child)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    value
        .as_str()
        .map(super::test_expression_has_unbound_needs)
        .transpose()
        .map(|value| value.unwrap_or(false))
}
