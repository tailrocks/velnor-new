use std::collections::{BTreeMap, BTreeSet};

use crate::{RenderError, RenderedFile};
use saphyr::Yaml;

use super::{WorkflowRoutingPolicy, parse::parse_final_workflow};

#[derive(Debug)]
struct WorkflowEntry {
    path: String,
    supports_workflow_call: bool,
    calls: Vec<ReusableCall>,
}

#[derive(Debug)]
struct ReusableCall {
    job_id: String,
    uses: String,
    target: String,
}

/// Parse every final workflow and inspect every job-level runner and reusable call.
pub(super) fn validate_final_workflow_inventory(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let mut workflows = BTreeMap::new();
    for file in files.iter().filter(|file| is_workflow_path(&file.path)) {
        let document = parse_final_workflow(file)?;
        let entry = inspect_workflow(&file.path, &document, policy)?;
        if workflows.insert(file.path.clone(), entry).is_some() {
            return Err(invalid(format!("duplicate_workflow_path:{}", file.path)));
        }
    }
    validate_reusable_calls(&workflows)
}

fn inspect_workflow(
    path: &str,
    document: &Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
) -> Result<WorkflowEntry, RenderError> {
    if untagged(document).as_mapping().is_none() {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    }
    let Some(jobs) = mapping_get(document, "jobs").and_then(Yaml::as_mapping) else {
        return Err(invalid(format!("workflow_jobs_missing:{path}")));
    };
    let supports_workflow_call = supports_workflow_call(document);
    let mut job_ids = BTreeSet::new();
    let mut calls = Vec::new();
    for (job_id, job) in jobs {
        let Some(job_id) = scalar_text(job_id) else {
            return Err(invalid(format!("workflow_job_id_not_scalar:{path}")));
        };
        if !job_ids.insert(job_id.to_owned()) {
            return Err(invalid(format!("duplicate_workflow_job:{path}:{job_id}")));
        }
        let Some(fields) = untagged(job).as_mapping() else {
            return Err(invalid(format!("workflow_job_not_map:{path}:{job_id}")));
        };
        if fields.keys().any(|key| scalar_text(key).is_none()) {
            return Err(invalid(format!(
                "workflow_job_field_not_scalar:{path}:{job_id}"
            )));
        }
        let reusable = mapping_get(job, "uses");
        let runs_on = mapping_get(job, "runs-on");
        if let Some(uses) = reusable {
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
            match local_workflow_target(uses, path, job_id)? {
                Some(target) => calls.push(ReusableCall {
                    job_id: job_id.to_owned(),
                    uses: uses.to_owned(),
                    target,
                }),
                None if policy.is_scale_set() => {
                    return Err(invalid(format!(
                        "unsupported_external_reusable_workflow:{path}:{job_id}:{uses}"
                    )));
                }
                None => {}
            }
            continue;
        }
        let Some(runs_on) = runs_on else {
            return Err(invalid(format!(
                "workflow_job_runs_on_missing:{path}:{job_id}"
            )));
        };
        if policy.is_scale_set() {
            validate_scale_set_runner(runs_on, policy, path, job_id)?;
        }
        let _condition = mapping_get(job, "if").and_then(scalar_text);
    }
    Ok(WorkflowEntry {
        path: path.to_owned(),
        supports_workflow_call,
        calls,
    })
}

fn validate_scale_set_runner(
    runs_on: &Yaml<'_>,
    policy: &WorkflowRoutingPolicy,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let WorkflowRoutingPolicy::ScaleSet(selector) = policy else {
        return Ok(());
    };
    let labels = match untagged(runs_on) {
        Yaml::Sequence(values) => values
            .iter()
            .map(|value| {
                scalar_text(value).ok_or_else(|| {
                    invalid(format!("workflow_runner_label_not_scalar:{path}:{job_id}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(invalid(format!(
                "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
            )));
        }
    };
    if labels
        != selector
            .labels()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        return Err(invalid(format!(
            "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
        )));
    }
    Ok(())
}

fn local_workflow_target(
    uses: &str,
    path: &str,
    job_id: &str,
) -> Result<Option<String>, RenderError> {
    if !uses.starts_with("./.github/workflows/") {
        return Ok(None);
    }
    let target = uses.strip_prefix("./").ok_or_else(|| {
        invalid(format!(
            "invalid_local_reusable_workflow:{path}:{job_id}:{uses}"
        ))
    })?;
    if target.contains("..") || target.contains('@') || !is_workflow_path(target) {
        return Err(invalid(format!(
            "invalid_local_reusable_workflow:{path}:{job_id}:{uses}"
        )));
    }
    Ok(Some(target.to_owned()))
}

fn validate_reusable_calls(workflows: &BTreeMap<String, WorkflowEntry>) -> Result<(), RenderError> {
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for workflow in workflows.values() {
        for call in &workflow.calls {
            let Some(callee) = workflows.get(&call.target) else {
                return Err(invalid(format!(
                    "unresolved_local_reusable_workflow:{}:{}",
                    workflow.path, call.uses
                )));
            };
            if !callee.supports_workflow_call {
                return Err(invalid(format!(
                    "local_reusable_workflow_call_missing:{}:{}:{}",
                    workflow.path, call.job_id, call.target
                )));
            }
            edges
                .entry(workflow.path.clone())
                .or_default()
                .push(callee.path.clone());
        }
    }
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    for workflow in workflows.keys() {
        visit_calls(workflow, &edges, &mut active, &mut complete)?;
    }
    Ok(())
}

fn visit_calls(
    workflow: &str,
    edges: &BTreeMap<String, Vec<String>>,
    active: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> Result<(), RenderError> {
    if complete.contains(workflow) {
        return Ok(());
    }
    if !active.insert(workflow.to_owned()) {
        return Err(invalid(format!("reusable_workflow_cycle:{workflow}")));
    }
    if let Some(callees) = edges.get(workflow) {
        for callee in callees {
            visit_calls(callee, edges, active, complete)?;
        }
    }
    active.remove(workflow);
    complete.insert(workflow.to_owned());
    Ok(())
}

fn supports_workflow_call(document: &Yaml<'_>) -> bool {
    let Some(on) = mapping_get(document, "on") else {
        return false;
    };
    match untagged(on) {
        Yaml::Mapping(_) => mapping_get(on, "workflow_call").is_some(),
        Yaml::Sequence(events) => events
            .iter()
            .any(|event| scalar_text(event) == Some("workflow_call")),
        _ => scalar_text(on) == Some("workflow_call"),
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
