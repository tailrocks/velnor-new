use std::collections::{BTreeMap, HashSet};

use super::super::ApprovedJobAuthorityPolicy;
use crate::error::HostError;

pub(super) fn validate_closure(policy: &ApprovedJobAuthorityPolicy) -> Result<(), HostError> {
    let content: BTreeMap<_, _> = policy
        .workflows
        .iter()
        .map(|workflow| {
            (
                (workflow.repository_id, workflow.path.as_str()),
                workflow.content_sha256.as_str(),
            )
        })
        .collect();
    for workflow in &policy.workflows {
        for call in &workflow.calls {
            if content.get(&(call.repository_id, call.path.as_str()))
                != Some(&call.content_sha256.as_str())
            {
                return Err(HostError::Config);
            }
        }
        for job in &workflow.jobs {
            if !content.contains_key(&(workflow.repository_id, job.root_path.as_str()))
                || !reachable(
                    policy,
                    workflow.repository_id,
                    &job.root_path,
                    &job.workflow_path,
                )
            {
                return Err(HostError::Config);
            }
        }
    }
    Ok(())
}

fn reachable(
    policy: &ApprovedJobAuthorityPolicy,
    repository_id: i64,
    root: &str,
    target: &str,
) -> bool {
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(path) = pending.pop() {
        if path == target {
            return true;
        }
        if !seen.insert(path) {
            continue;
        }
        if let Some(workflow) = policy
            .workflows
            .iter()
            .find(|workflow| workflow.repository_id == repository_id && workflow.path == path)
        {
            pending.extend(workflow.calls.iter().map(|call| call.path.as_str()));
        }
    }
    false
}

pub(super) fn validate_acyclic_closure(
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<(), HostError> {
    let mut complete = HashSet::new();
    for workflow in &policy.workflows {
        if !visit_workflow(
            policy,
            workflow.repository_id,
            &workflow.path,
            &mut complete,
            &mut HashSet::new(),
        ) {
            return Err(HostError::Config);
        }
    }
    Ok(())
}

fn visit_workflow(
    policy: &ApprovedJobAuthorityPolicy,
    repository_id: i64,
    path: &str,
    complete: &mut HashSet<(i64, String)>,
    active: &mut HashSet<(i64, String)>,
) -> bool {
    let key = (repository_id, path.to_owned());
    if complete.contains(&key) {
        return true;
    }
    if !active.insert(key.clone()) {
        return false;
    }
    let valid = policy
        .workflows
        .iter()
        .find(|workflow| workflow.repository_id == repository_id && workflow.path == path)
        .is_some_and(|workflow| {
            workflow.calls.iter().all(|call| {
                visit_workflow(policy, call.repository_id, &call.path, complete, active)
            })
        });
    active.remove(&key);
    if valid {
        complete.insert(key);
    }
    valid
}
