use std::collections::BTreeMap;

use saphyr::Yaml;

use crate::{RenderError, RenderedFile};

#[path = "workflow_routing_authority_triggers.rs"]
mod triggers;
#[path = "workflow_routing_authority_values.rs"]
mod values;

/// Trigger facts found in one final workflow file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityTrigger {
    /// GitHub event name.
    pub event: String,
    /// Exact branch filters.
    pub branch_filters: Vec<String>,
    /// Exact tag filters.
    pub tag_filters: Vec<String>,
    /// Exact pull-request base branch filters.
    pub pull_request_base_branches: Vec<String>,
}

/// One local reusable-workflow call edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityCall {
    /// Caller job key.
    pub job_key: String,
    /// Repository-relative workflow path.
    pub path: String,
}

/// Permission facts from one final workflow job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityPermissions {
    /// True when workflow or job YAML declares permissions.
    pub declared: bool,
    /// Permission scope values.
    pub scopes: BTreeMap<String, String>,
}

/// Exact concurrency source for one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityConcurrency {
    /// Source location.
    pub scope: String,
    /// Exact group expression.
    pub group: String,
    /// Exact cancel expression.
    pub cancel_in_progress: String,
}

/// One job in a final workflow file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityJob {
    /// Exact job key.
    pub job_key: String,
    /// Job dependencies.
    pub needs: Vec<String>,
    /// Permission declaration and scopes selected for this job.
    pub permissions: AuthorityPermissions,
    /// Secret names found in this job's YAML values.
    pub secrets: Vec<String>,
    /// Exact environment names.
    pub environments: Vec<String>,
    /// Exact condition source.
    pub condition: Option<String>,
    /// Concurrency source selected for this job.
    pub concurrency: Option<AuthorityConcurrency>,
    /// True when the job has a matrix.
    pub has_matrix: bool,
    /// Local reusable-workflow target, when present.
    pub reusable_call: Option<String>,
}

/// Authority facts read from one final workflow file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityWorkflow {
    /// Repository-relative workflow path.
    pub path: String,
    /// Trigger facts.
    pub triggers: Vec<AuthorityTrigger>,
    /// Direct local reusable-workflow calls.
    pub calls: Vec<AuthorityCall>,
    /// Workflow-level permission declaration.
    pub workflow_permissions: Option<BTreeMap<String, String>>,
    /// Workflow-level concurrency source.
    pub workflow_concurrency: Option<AuthorityConcurrency>,
    /// Jobs keyed by their exact YAML job key.
    pub jobs: Vec<AuthorityJob>,
}

/// Read authority facts with the YAML parser used by routing checks.
///
/// # Errors
///
/// The function rejects events and authority values that it cannot use.
pub fn extract_workflow_authority(
    files: &[RenderedFile],
) -> Result<Vec<AuthorityWorkflow>, RenderError> {
    let mut workflows = Vec::new();
    for file in files.iter().filter(|file| is_workflow_path(&file.path)) {
        let document = super::parse_authority_document(file)?;
        workflows.push(extract_one(&file.path, &document)?);
    }
    workflows.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(workflows)
}

fn extract_one(path: &str, document: &Yaml<'_>) -> Result<AuthorityWorkflow, RenderError> {
    let triggers = triggers::extract_triggers(path, get(document, "on"))?;
    let workflow_permissions = get(document, "permissions")
        .map(|value| values::permission_map(value, path, "workflow"))
        .transpose()?;
    let workflow_concurrency = get(document, "concurrency")
        .map(|value| values::concurrency(value, "workflow", path, "workflow"))
        .transpose()?;
    let inherited_secrets = get(document, "env")
        .map(|value| values::collect_secrets(value, path, "workflow"))
        .transpose()?
        .unwrap_or_default();
    let jobs_value =
        get(document, "jobs").ok_or_else(|| invalid(format!("workflow_jobs_missing:{path}")))?;
    let jobs_map = untagged(jobs_value)
        .as_mapping()
        .ok_or_else(|| invalid(format!("workflow_jobs_not_map:{path}")))?;
    let mut jobs = Vec::new();
    let mut calls = Vec::new();
    for (key, value) in jobs_map {
        let key =
            scalar(key).ok_or_else(|| invalid(format!("workflow_job_id_not_scalar:{path}")))?;
        let _fields = untagged(value)
            .as_mapping()
            .ok_or_else(|| invalid(format!("workflow_job_not_map:{path}:{key}")))?;
        let local = get(value, "uses")
            .map(|uses| reusable_target(uses, path, key))
            .transpose()?
            .flatten();
        if let Some(target) = &local {
            calls.push(AuthorityCall {
                job_key: key.to_owned(),
                path: target.clone(),
            });
        }
        let own_permissions = get(value, "permissions")
            .map(|permission| values::permission_map(permission, path, key))
            .transpose()?;
        let effective_permissions = own_permissions
            .clone()
            .or_else(|| workflow_permissions.clone());
        let permissions = AuthorityPermissions {
            declared: effective_permissions.is_some(),
            scopes: effective_permissions.unwrap_or_default(),
        };
        let secrets = values::merge_secrets(
            &inherited_secrets,
            &values::collect_secrets(value, path, key)?,
        );
        let environments = values::extract_environments(get(value, "environment"), path, key)?;
        let condition = values::optional_scalar(get(value, "if"), path, key, "condition")?;
        let job_concurrency = get(value, "concurrency")
            .map(|entry| values::concurrency(entry, "job", path, key))
            .transpose()?;
        let needs = values::extract_needs(get(value, "needs"), path, key)?;
        let has_matrix = get(value, "strategy")
            .and_then(|strategy| get(strategy, "matrix"))
            .is_some();
        jobs.push(AuthorityJob {
            job_key: key.to_owned(),
            needs,
            permissions,
            secrets,
            environments,
            condition,
            concurrency: job_concurrency.or_else(|| workflow_concurrency.clone()),
            has_matrix,
            reusable_call: local,
        });
    }
    jobs.sort_by(|left, right| left.job_key.cmp(&right.job_key));
    calls.sort_by(|left, right| (&left.job_key, &left.path).cmp(&(&right.job_key, &right.path)));
    Ok(AuthorityWorkflow {
        path: path.to_owned(),
        triggers,
        calls,
        workflow_permissions,
        workflow_concurrency,
        jobs,
    })
}

fn reusable_target(
    value: &Yaml<'_>,
    workflow_path: &str,
    job_key: &str,
) -> Result<Option<String>, RenderError> {
    let Some(uses) = scalar(value) else {
        return Err(invalid(format!(
            "workflow_reusable_call_unresolved:{workflow_path}:{job_key}"
        )));
    };
    let Some(target) = uses.strip_prefix("./") else {
        return Err(invalid(format!(
            "workflow_reusable_call_external:{workflow_path}:{job_key}"
        )));
    };
    if !target.starts_with(".github/workflows/") || target.contains('@') {
        return Err(invalid(format!(
            "workflow_reusable_call_invalid:{workflow_path}:{job_key}"
        )));
    }
    Ok(Some(target.to_owned()))
}

fn get<'a, 'input>(value: &'a Yaml<'input>, name: &str) -> Option<&'a Yaml<'input>> {
    untagged(value)
        .as_mapping()?
        .iter()
        .find_map(|(key, value)| (scalar(key) == Some(name)).then_some(value))
}

fn scalar<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    untagged(value).as_str()
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
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_authority_tests.rs"]
mod tests;
