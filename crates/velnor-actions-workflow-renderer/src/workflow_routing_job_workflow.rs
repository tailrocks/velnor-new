use std::collections::BTreeSet;

use saphyr::{Mapping as ParsedMapping, Yaml as ParsedYaml};
use velnor_actions_contract::{PermissionLevel, Permissions};

use super::{invalid, parse};
use crate::{
    RenderError, RenderedFile,
    lane_share::job_workflow_extract::{
        CALLER_INPUT_REQUIRED, CALLER_INPUT_TYPE, CALLER_INPUTS, JobWorkflowRelocation,
    },
    yaml::Yaml,
};

/// Validate generated callers and callees against typed, source-derived snapshots.
pub(super) fn validate_relocated_job_workflows(
    parent_yaml: &Yaml,
    shared_files: &[RenderedFile],
    relocations: &[JobWorkflowRelocation],
) -> Result<(), RenderError> {
    if relocations.is_empty() {
        return Ok(());
    }
    let parent_jobs = parent_jobs(parent_yaml)?;
    let mut caller_ids = BTreeSet::new();
    let mut callee_paths = BTreeSet::new();
    for relocation in relocations {
        let prepared = &relocation.prepared;
        prepared.validate_source()?;
        if !caller_ids.insert(prepared.source_id.as_str())
            || !callee_paths.insert(prepared.callee_path.as_str())
        {
            return Err(invalid(format!(
                "job_workflow_duplicate_relocation:{}:{}",
                prepared.source_id, prepared.callee_path
            )));
        }
        crate::guard::validate_tree_path(&prepared.callee_path)?;
        validate_caller(parent_jobs, relocation)?;
        validate_callee(shared_files, relocation)?;
    }
    Ok(())
}

fn parent_jobs(parent_yaml: &Yaml) -> Result<&[(String, Yaml)], RenderError> {
    let Yaml::Map(fields) = parent_yaml else {
        return Err(invalid("job_workflow_parent_not_map".to_owned()));
    };
    let jobs = unique_field(fields, "jobs", "job_workflow_parent_jobs")?
        .ok_or_else(|| invalid("job_workflow_parent_jobs_missing".to_owned()))?;
    let Yaml::Map(jobs) = jobs else {
        return Err(invalid("job_workflow_parent_jobs_not_map".to_owned()));
    };
    let mut ids = BTreeSet::new();
    for (job_id, _) in jobs {
        if !ids.insert(job_id.as_str()) {
            return Err(invalid(format!(
                "job_workflow_duplicate_parent_job:{job_id}"
            )));
        }
    }
    Ok(jobs)
}

fn validate_caller(
    parent_jobs: &[(String, Yaml)],
    relocation: &JobWorkflowRelocation,
) -> Result<(), RenderError> {
    let prepared = &relocation.prepared;
    let Some((_, actual)) = parent_jobs
        .iter()
        .find(|(job_id, _)| job_id == &prepared.source_id)
    else {
        return Err(invalid(format!(
            "job_workflow_parent_caller_missing:{}",
            prepared.source_id
        )));
    };
    let expected = source_caller_yaml(prepared);
    if relocation.expected_caller != expected || actual != &expected {
        return Err(invalid(format!(
            "job_workflow_parent_caller_mismatch:{}",
            prepared.source_id
        )));
    }
    Ok(())
}

fn source_caller_yaml(prepared: &crate::lane_share::job_workflow_extract::PreparedJob) -> Yaml {
    let source = &prepared.source_job;
    let inputs = CALLER_INPUTS
        .iter()
        .map(|input| (input.name.to_owned(), Yaml::str(input.expression)))
        .collect();
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(source.display_name.clone())),
        (
            "uses".to_owned(),
            Yaml::str(format!("./{}", prepared.callee_path)),
        ),
        ("with".to_owned(), Yaml::Map(inputs)),
    ];
    if let Some(permissions) = &source.permissions {
        fields.push(("permissions".to_owned(), permissions_yaml(permissions)));
    }
    if let Some(cache_mode) = &source.cache_mode {
        fields.push(Yaml::cache_mode_field(cache_mode));
    }
    if !source.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(source.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(condition) = &source.condition {
        fields.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    Yaml::Map(fields)
}

fn permissions_yaml(permissions: &Permissions) -> Yaml {
    let mut entries = vec![(
        "contents".to_owned(),
        Yaml::str(permission_level(permissions.contents)),
    )];
    for (name, level) in [
        ("actions", permissions.actions),
        ("pull-requests", permissions.pull_requests),
        ("id-token", permissions.id_token),
        ("packages", permissions.packages),
    ] {
        if level != PermissionLevel::None {
            entries.push((name.to_owned(), Yaml::str(permission_level(level))));
        }
    }
    Yaml::Map(entries)
}

fn permission_level(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
        PermissionLevel::None => "none",
    }
}

fn validate_callee(
    shared_files: &[RenderedFile],
    relocation: &JobWorkflowRelocation,
) -> Result<(), RenderError> {
    let prepared = &relocation.prepared;
    let expected = &relocation.expected_callee_file;
    if expected.path != prepared.callee_path {
        return Err(invalid(format!(
            "job_workflow_callee_path_mismatch:{}",
            prepared.source_id
        )));
    }
    let mut candidates = shared_files
        .iter()
        .filter(|file| file.path == prepared.callee_path);
    let Some(actual) = candidates.next() else {
        return Err(invalid(format!(
            "job_workflow_callee_missing:{}",
            prepared.callee_path
        )));
    };
    if candidates.next().is_some() || actual != expected {
        return Err(invalid(format!(
            "job_workflow_callee_mismatch:{}",
            prepared.callee_path
        )));
    }
    validate_callee_shape(expected, &prepared.source_id)
}

fn validate_callee_shape(file: &RenderedFile, source_id: &str) -> Result<(), RenderError> {
    let document = parse::parse_final_workflow(file)?;
    let on = parsed_field(&document, "on", "job_workflow_callee_on")?
        .ok_or_else(|| invalid("job_workflow_callee_on_missing".to_owned()))?;
    let workflow_call = parsed_field(on, "workflow_call", "job_workflow_workflow_call")?
        .ok_or_else(|| invalid("job_workflow_workflow_call_missing".to_owned()))?;
    let inputs = parsed_field(workflow_call, "inputs", "job_workflow_inputs")?
        .ok_or_else(|| invalid("job_workflow_inputs_missing".to_owned()))?;
    if !parsed_has_exact_keys(workflow_call, &["inputs"])
        || !parsed_has_exact_keys(on, &["workflow_call"])
        || !valid_inputs(inputs)
    {
        return Err(invalid(format!("job_workflow_inputs_invalid:{source_id}")));
    }
    let jobs = parsed_field(&document, "jobs", "job_workflow_callee_jobs")?
        .ok_or_else(|| invalid("job_workflow_callee_jobs_missing".to_owned()))?;
    let Some(entries) = parsed_map(jobs) else {
        return Err(invalid("job_workflow_callee_jobs_not_map".to_owned()));
    };
    let sole_job_id = entries
        .iter()
        .next()
        .and_then(|(job_id, _)| parsed_scalar(job_id));
    if entries.len() != 1 || sole_job_id != Some(source_id) {
        return Err(invalid(format!(
            "job_workflow_inner_job_mismatch:{source_id}"
        )));
    }
    Ok(())
}

fn valid_inputs(inputs: &ParsedYaml<'_>) -> bool {
    let Some(entries) = parsed_map(inputs) else {
        return false;
    };
    entries.len() == CALLER_INPUTS.len()
        && CALLER_INPUTS.iter().all(|input| {
            parsed_field(inputs, input.name, "job_workflow_input")
                .ok()
                .flatten()
                .is_some_and(|definition| {
                    parsed_has_exact_keys(definition, &["type", "required"])
                        && parsed_field(definition, "type", "job_workflow_input_type")
                            .ok()
                            .flatten()
                            .and_then(parsed_scalar)
                            == Some(CALLER_INPUT_TYPE)
                        && parsed_field(definition, "required", "job_workflow_input_required")
                            .ok()
                            .flatten()
                            .and_then(ParsedYaml::as_bool)
                            == Some(CALLER_INPUT_REQUIRED)
                })
        })
}

fn parsed_has_exact_keys(value: &ParsedYaml<'_>, expected: &[&str]) -> bool {
    parsed_map(value).is_some_and(|entries| {
        entries.len() == expected.len()
            && expected.iter().all(|name| {
                parsed_field(value, name, "job_workflow_keys")
                    .ok()
                    .flatten()
                    .is_some()
            })
    })
}

fn parsed_field<'node, 'input>(
    value: &'node ParsedYaml<'input>,
    name: &str,
    context: &str,
) -> Result<Option<&'node ParsedYaml<'input>>, RenderError> {
    let Some(entries) = parsed_map(value) else {
        return Err(invalid(format!("{context}_not_map")));
    };
    let mut matching = entries
        .iter()
        .filter(|(key, _)| parsed_scalar(key) == Some(name));
    let first = matching.next().map(|(_, value)| value);
    if matching.next().is_some() {
        return Err(invalid(format!("{context}_duplicate:{name}")));
    }
    Ok(first)
}

fn parsed_map<'node, 'input>(
    value: &'node ParsedYaml<'input>,
) -> Option<&'node ParsedMapping<'input>> {
    value.as_mapping()
}

fn parsed_scalar<'node>(value: &'node ParsedYaml<'_>) -> Option<&'node str> {
    value.as_str()
}

fn unique_field<'a>(
    fields: &'a [(String, Yaml)],
    key: &str,
    context: &str,
) -> Result<Option<&'a Yaml>, RenderError> {
    let mut matching = fields.iter().filter(|(name, _)| name.as_str() == key);
    let value = matching.next().map(|(_, value)| value);
    if matching.next().is_some() {
        return Err(invalid(format!("{context}_duplicate:{key}")));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "workflow_routing_job_workflow_tests.rs"]
mod tests;
