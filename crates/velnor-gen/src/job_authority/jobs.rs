// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;
use velnor_model::RepoPath;
use velnor_workflows::{reads_secret_in_condition, reads_secret_in_text};

use super::{JobAuthorityError, inventory::Job, triggers};
use crate::workflow_triggers::parse_workflow;

pub(super) fn inventory_jobs(
    path: &RepoPath,
    text: &str,
    default_branch: &str,
) -> Result<(Vec<Job>, Vec<String>), JobAuthorityError> {
    let fail = |reason: String| JobAuthorityError::Workflow(path.clone(), reason);
    let document = parse_workflow(text).ok_or_else(|| fail("is not readable YAML".to_owned()))?;
    let on = document
        .get("on")
        .ok_or_else(|| fail("has no `on` key".to_owned()))?;
    let contexts = triggers::contexts(on, default_branch).map_err(&fail)?;
    let declared = jobs_of(&document).map_err(&fail)?;
    let workflow_permissions = document.get("permissions");
    let workflow_reads_secret = document.get("env").is_some_and(reads_secret);
    let mut jobs = Vec::new();
    let mut omitted = Vec::new();
    let uncovered = triggers::uncovered(on);
    if !uncovered.is_empty() {
        omitted.push(format!(
            "job-authority: {path} also triggers on {}, which the inventory does not cover; the host refuses those runs",
            uncovered.join(" and ")
        ));
    }
    for (key, job) in declared {
        if job.get("uses").is_some() {
            return Err(fail(format!("job {key} calls a reusable workflow")));
        }
        let permissions = permissions_of(job.get("permissions").or(workflow_permissions))
            .map_err(|reason| fail(format!("job {key}: {reason}")))?;
        if workflow_reads_secret || needs_publication_authority(job, &permissions) {
            omitted.push(format!(
                "job-authority: {path} job {key} is left out of the inventory: it writes, uses secrets or names an environment, which the verification role refuses"
            ));
            continue;
        }
        jobs.push(Job::verification(
            path,
            key,
            &contexts,
            permissions,
            needs_of(job),
        ));
    }
    Ok((jobs, omitted))
}

fn jobs_of(document: &Value) -> Result<BTreeMap<&str, &Value>, String> {
    document
        .get("jobs")
        .and_then(Value::as_object)
        .map(|jobs| jobs.iter().map(|(key, job)| (key.as_str(), job)).collect())
        .ok_or_else(|| "has no `jobs` mapping".to_owned())
}

fn permissions_of(value: Option<&Value>) -> Result<BTreeMap<String, String>, String> {
    let declared = value
        .and_then(Value::as_object)
        .ok_or("declares no permissions mapping")?;
    declared
        .iter()
        .map(|(name, level)| match level.as_str() {
            Some(level @ ("none" | "read" | "write")) => Ok((name.clone(), level.to_owned())),
            _ => Err(format!("permission {name} is not none, read or write")),
        })
        .collect()
}

fn needs_publication_authority(job: &Value, permissions: &BTreeMap<String, String>) -> bool {
    permissions.values().any(|level| level == "write")
        || job.get("environment").is_some()
        || reads_secret(job)
}

fn reads_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => reads_secret_in_text(text),
        Value::Array(items) => items.iter().any(reads_secret),
        Value::Object(map) => map.iter().any(|(key, value)| match value {
            Value::String(text) if key == "if" => reads_secret_in_condition(text),
            _ => reads_secret(value),
        }),
        _ => false,
    }
}

fn needs_of(job: &Value) -> Vec<String> {
    let mut needs: Vec<String> = match job.get("needs") {
        Some(Value::String(name)) => vec![name.clone()],
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    needs.sort();
    needs.dedup();
    needs
}
