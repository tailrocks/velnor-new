use std::collections::{BTreeMap, BTreeSet};

use crate::{RenderError, RenderedFile};
use saphyr::Yaml;

use super::{WorkflowRoutingPolicy, calls, container, labels, matrix, parse::parse_final_workflow};
use velnor_actions_contract::{ExecutionMode, RunsOn};

#[derive(Debug)]
pub(super) struct WorkflowEntry {
    pub(super) path: String,
    pub(super) supports_workflow_call: bool,
    pub(super) calls: Vec<ReusableCall>,
}

#[derive(Debug)]
pub(super) struct ReusableCall {
    pub(super) job_id: String,
    pub(super) uses: String,
    pub(super) target: String,
}

/// Parse every final workflow and inspect every job-level runner and reusable call.
pub(super) fn validate_final_workflow_inventory(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&BTreeMap<String, velnor_actions_contract::Job>>,
) -> Result<(), RenderError> {
    let mut workflows = BTreeMap::new();
    for file in files.iter().filter(|file| is_workflow_path(&file.path)) {
        let document = parse_final_workflow(file)?;
        let typed_jobs = (file.path == ".github/workflows/ci.yml")
            .then_some(typed_jobs)
            .flatten();
        let entry = inspect_workflow(&file.path, &document, policy, typed_jobs)?;
        if workflows.insert(file.path.clone(), entry).is_some() {
            return Err(invalid(format!("duplicate_workflow_path:{}", file.path)));
        }
    }
    calls::validate_reusable_calls(&workflows)
}

fn inspect_workflow(
    path: &str,
    document: &Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&BTreeMap<String, velnor_actions_contract::Job>>,
) -> Result<WorkflowEntry, RenderError> {
    if untagged(document).as_mapping().is_none() {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    }
    let Some(jobs) = mapping_get(document, "jobs").and_then(Yaml::as_mapping) else {
        return Err(invalid(format!("workflow_jobs_missing:{path}")));
    };
    let supports_workflow_call = calls::supports_workflow_call(document);
    let mut job_ids = BTreeSet::new();
    let mut calls = Vec::new();
    for (job_id, job) in jobs {
        let Some(job_id) = scalar_text(job_id) else {
            return Err(invalid(format!("workflow_job_id_not_scalar:{path}")));
        };
        if !job_ids.insert(job_id.to_owned()) {
            return Err(invalid(format!("duplicate_workflow_job:{path}:{job_id}")));
        }
        if let Some(call) = inspect_workflow_job(path, job_id, job, policy, typed_jobs)? {
            calls.push(call);
        }
    }
    Ok(WorkflowEntry {
        path: path.to_owned(),
        supports_workflow_call,
        calls,
    })
}

fn inspect_workflow_job(
    path: &str,
    job_id: &str,
    job: &Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&BTreeMap<String, velnor_actions_contract::Job>>,
) -> Result<Option<ReusableCall>, RenderError> {
    let Some(fields) = untagged(job).as_mapping() else {
        return Err(invalid(format!("workflow_job_not_map:{path}:{job_id}")));
    };
    if fields.keys().any(|key| scalar_text(key).is_none()) {
        return Err(invalid(format!(
            "workflow_job_field_not_scalar:{path}:{job_id}"
        )));
    }
    let runs_on = mapping_get(job, "runs-on");
    let container_value = mapping_get(job, "container");
    if let Some(uses) = mapping_get(job, "uses") {
        return inspect_reusable_call(path, job_id, uses, runs_on, container_value, policy);
    }
    let Some(runs_on) = runs_on else {
        return Err(invalid(format!(
            "workflow_job_runs_on_missing:{path}:{job_id}"
        )));
    };
    let typed_job = typed_jobs.and_then(|jobs| jobs.get(job_id));
    if super::typed_jobs::accept_typed_native_check(
        scalar_text(runs_on),
        mapping_get(job, "if").and_then(scalar_text),
        container_value.is_some(),
        typed_job,
        policy,
        path,
        job_id,
    )? {
        return Ok(None);
    }
    validate_runner(fields, runs_on, container_value, policy, path, job_id)?;
    Ok(None)
}

fn inspect_reusable_call(
    path: &str,
    job_id: &str,
    uses: &Yaml<'_>,
    runs_on: Option<&Yaml<'_>>,
    container_value: Option<&Yaml<'_>>,
    policy: &WorkflowRoutingPolicy,
) -> Result<Option<ReusableCall>, RenderError> {
    let Some(uses) = scalar_text(uses) else {
        return Err(invalid(format!(
            "workflow_job_uses_not_scalar:{path}:{job_id}"
        )));
    };
    if runs_on.is_some() {
        return Err(invalid(format!(
            "reusable_workflow_job_has_runs_on:{path}:{job_id}"
        )));
    }
    if container_value.is_some() {
        return Err(invalid(format!(
            "reusable_workflow_job_has_container:{path}:{job_id}"
        )));
    }
    match calls::local_workflow_target(uses, path, job_id)? {
        Some(target) => Ok(Some(ReusableCall {
            job_id: job_id.to_owned(),
            uses: uses.to_owned(),
            target,
        })),
        None if policy.selector().is_some() => Err(invalid(format!(
            "unsupported_external_reusable_workflow:{path}:{job_id}:{uses}"
        ))),
        None => Ok(None),
    }
}

fn validate_runner(
    fields: &saphyr::Mapping<'_>,
    runs_on: &Yaml<'_>,
    container_value: Option<&Yaml<'_>>,
    policy: &WorkflowRoutingPolicy,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let selector = policy.selector();
    let mode = Some(policy.mode);
    let choice = resolve_runner_choice(fields, runs_on, selector, path, job_id)?;
    validate_runner_choice(choice, container_value, policy, mode, path, job_id)
}

enum RunnerChoice {
    Selector,
    Hosted(Vec<String>),
    Unresolved,
}

fn resolve_runner_choice(
    fields: &saphyr::Mapping<'_>,
    runs_on: &Yaml<'_>,
    selector: Option<&velnor_actions_contract::ScaleSetSelector>,
    path: &str,
    job_id: &str,
) -> Result<RunnerChoice, RenderError> {
    match untagged(runs_on) {
        Yaml::Sequence(values) => {
            let labels = values.iter().map(scalar_text).collect::<Option<Vec<_>>>();
            let Some(labels) = labels else {
                return Ok(RunnerChoice::Unresolved);
            };
            if selector.is_some_and(|selector| {
                labels
                    == selector
                        .labels()
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
            }) {
                Ok(RunnerChoice::Selector)
            } else {
                Ok(RunnerChoice::Hosted(
                    labels.into_iter().map(str::to_owned).collect(),
                ))
            }
        }
        Yaml::Value(_) | Yaml::Representation(..) => {
            let text = scalar_text(runs_on).unwrap_or_default();
            if matrix::is_matrix_expression(text) {
                let converted = routing_fields(fields).ok_or_else(|| {
                    invalid(format!(
                        "matrix_runner_values_invalid:{path}:{job_id}:{text}"
                    ))
                })?;
                let labels = matrix::matrix_runner_labels(&converted, text, path, job_id)?;
                if let Some(selector) = selector
                    && labels.iter().any(|label| label == &selector.token())
                {
                    if labels.len() == 1 && labels[0] == selector.token() {
                        return Ok(RunnerChoice::Unresolved);
                    }
                    return Err(invalid(format!(
                        "matrix_runner_policy_mixed:{path}:{job_id}"
                    )));
                }
                Ok(RunnerChoice::Hosted(labels))
            } else {
                match RunsOn::parse(text) {
                    Ok(RunsOn::Hosted(label)) => Ok(RunnerChoice::Hosted(vec![label])),
                    Ok(RunsOn::ScaleSet(existing))
                        if selector.is_some_and(|selector| existing == *selector) =>
                    {
                        Ok(RunnerChoice::Selector)
                    }
                    _ => Ok(RunnerChoice::Unresolved),
                }
            }
        }
        _ => Ok(RunnerChoice::Unresolved),
    }
}

fn routing_fields(fields: &saphyr::Mapping<'_>) -> Option<Vec<(String, crate::yaml::Yaml)>> {
    fields
        .iter()
        .map(|(key, value)| Some((scalar_text(key)?.to_owned(), to_routing_yaml(value)?)))
        .collect()
}

fn validate_runner_choice(
    choice: RunnerChoice,
    container_value: Option<&Yaml<'_>>,
    policy: &WorkflowRoutingPolicy,
    mode: Option<ExecutionMode>,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    match choice {
        RunnerChoice::Selector => {
            if policy.selector().is_none() {
                return Err(invalid(format!(
                    "scale_set_selector_unconfigured:{path}:{job_id}"
                )));
            }
            if policy.image().is_none() {
                return Err(invalid(format!(
                    "job_container_image_missing:{path}:{job_id}"
                )));
            }
            let Some(container_value) = container_value else {
                return Err(invalid(format!("job_container_missing:{path}:{job_id}")));
            };
            container::validate_parsed_container(container_value, path, job_id)?;
            Ok(())
        }
        RunnerChoice::Hosted(labels) => {
            if mode == Some(ExecutionMode::ScaleSet) {
                return Err(invalid(format!(
                    "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
                )));
            }
            labels::validate_preserved_hosted_labels(&labels, path, job_id)?;
            Ok(())
        }
        RunnerChoice::Unresolved if policy.is_scale_set() => Err(invalid(format!(
            "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
        ))),
        RunnerChoice::Unresolved => Err(invalid(format!(
            "workflow_runner_unresolved:{path}:{job_id}"
        ))),
    }
}

fn to_routing_yaml(value: &Yaml<'_>) -> Option<crate::yaml::Yaml> {
    use crate::yaml::Yaml as RoutingYaml;
    match untagged(value) {
        Yaml::Value(_) | Yaml::Representation(..) => {
            if let Some(text) = value.as_str() {
                Some(RoutingYaml::str(text))
            } else if let Some(boolean) = value.as_bool() {
                Some(RoutingYaml::Bool(boolean))
            } else {
                value.as_integer().map(RoutingYaml::Int)
            }
        }
        Yaml::Sequence(values) => values
            .iter()
            .map(to_routing_yaml)
            .collect::<Option<Vec<_>>>()
            .map(RoutingYaml::Seq),
        Yaml::Mapping(entries) => entries
            .iter()
            .map(|(key, value)| Some((scalar_text(key)?.to_owned(), to_routing_yaml(value)?)))
            .collect::<Option<Vec<_>>>()
            .map(RoutingYaml::Map),
        _ => None,
    }
}

fn scalar_text<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    untagged(value).as_str()
}

fn mapping_get<'a, 'input>(value: &'a Yaml<'input>, key: &str) -> Option<&'a Yaml<'input>> {
    untagged(value)
        .as_mapping()?
        .iter()
        .find_map(|(name, value)| (scalar_text(name) == Some(key)).then_some(value))
}

fn untagged<'a, 'input>(value: &'a Yaml<'input>) -> &'a Yaml<'input> {
    match value {
        Yaml::Tagged(_, inner) => untagged(inner),
        _ => value,
    }
}

fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_inventory_tests.rs"]
mod tests;
