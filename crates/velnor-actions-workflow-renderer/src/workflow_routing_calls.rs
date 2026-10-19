use std::collections::{BTreeMap, BTreeSet};

use crate::RenderError;
use saphyr::Yaml;

use super::{invalid, inventory::WorkflowEntry};

pub(super) fn local_workflow_target(
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

pub(super) fn validate_reusable_calls(
    workflows: &BTreeMap<String, WorkflowEntry>,
) -> Result<(), RenderError> {
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

fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

pub(super) fn supports_workflow_call(document: &Yaml<'_>) -> bool {
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
