//! Shared placement policy for every generated workflow document.

use std::path::Path;
use velnor_actions_contract::{
    ExecutionMode, RunnerPlatform, RunsOn, ScaleSetSelector, runner_platform,
};

use crate::{RenderError, RenderedFile, yaml::Yaml};

/// Effective placement policy for a generated workflow document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRoutingPolicy {
    /// Keep each renderer's established runner placement.
    Preserve,
    /// Route every Linux x86-64 job to this validated selector.
    ScaleSet(ScaleSetSelector),
}

impl WorkflowRoutingPolicy {
    /// Resolve the document policy from the globally effective execution mode.
    #[must_use]
    pub fn from_mode(mode: ExecutionMode, selector: ScaleSetSelector) -> Self {
        match mode {
            ExecutionMode::ScaleSet => Self::ScaleSet(selector),
            ExecutionMode::Hosted | ExecutionMode::Both => Self::Preserve,
        }
    }

    /// True when all generated workflow documents must use the Scale Set.
    #[must_use]
    pub const fn is_scale_set(&self) -> bool {
        matches!(self, Self::ScaleSet(_))
    }
}

/// Use the selected policy for one typed workflow document.
pub(crate) fn route_workflow_yaml(
    path: &str,
    mut workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<Yaml, RenderError> {
    let WorkflowRoutingPolicy::ScaleSet(selector) = policy else {
        return Ok(workflow);
    };
    let scale_runs_on = Yaml::Flow(selector.labels().to_vec());
    let Yaml::Map(document) = &mut workflow else {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    };
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(key, _)| key == "jobs") else {
        return Err(invalid(format!("workflow_jobs_missing:{path}")));
    };
    for (job_id, job) in jobs {
        let Yaml::Map(fields) = job else {
            return Err(invalid(format!("workflow_job_not_map:{path}:{job_id}")));
        };
        let uses =
            fields
                .iter()
                .find(|(key, _)| key == "uses")
                .and_then(|(_, value)| match value {
                    Yaml::Str(text) | Yaml::Quoted(text) => Some(text.clone()),
                    Yaml::Annotated { value, .. } => Some(value.clone()),
                    _ => None,
                });
        let runs_on_index = fields.iter().position(|(key, _)| key == "runs-on");
        if let Some(uses) = uses {
            if runs_on_index.is_some() {
                return Err(invalid(format!(
                    "reusable_workflow_job_has_runs_on:{path}:{job_id}"
                )));
            }
            if !is_local_reusable_workflow(&uses) {
                return Err(invalid(format!(
                    "unsupported_external_reusable_workflow:{path}:{job_id}:{uses}"
                )));
            }
            continue;
        }
        let Some(runs_on_index) = runs_on_index else {
            return Err(invalid(format!(
                "workflow_job_runs_on_missing:{path}:{job_id}"
            )));
        };
        let replacement = match &fields[runs_on_index].1 {
            Yaml::Str(value) if let Some(key) = matrix_expression_key(value) => {
                let labels = matrix_runner_labels(fields, key, path, job_id)?;
                reject_unsupported_targets(&labels, path, job_id)?;
                scale_runs_on.clone()
            }
            Yaml::Str(value) => {
                let label = parsed_hosted_label(value, path, job_id)?;
                reject_unsupported_targets(&[label], path, job_id)?;
                scale_runs_on.clone()
            }
            Yaml::Flow(labels) if labels == selector.labels() => continue,
            _ => {
                return Err(invalid(format!(
                    "unsupported_runs_on_for_linux_amd64_scale_set:{path}:{job_id}"
                )));
            }
        };
        fields[runs_on_index].1 = replacement;
    }
    Ok(workflow)
}

/// Resolve job-level reusable workflow calls after all generated files exist.
///
/// GitHub expresses reusable jobs with four-space-indented `uses:` fields;
/// action-step `uses:` fields have greater indentation and are ignored. The
/// renderer owns these YAML bytes, so this check consumes only its canonical
/// emitted form.
///
/// # Errors
///
/// Invalid workflow bytes, unsupported external calls, and unresolved local calls fail.
pub fn validate_generated_workflow_calls(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    if !policy.is_scale_set() {
        return Ok(());
    }
    let workflows: std::collections::BTreeSet<&str> = files
        .iter()
        .filter(|file| is_workflow_path(&file.path))
        .map(|file| file.path.as_str())
        .collect();
    for file in files.iter().filter(|file| is_workflow_path(&file.path)) {
        let text = file.bytes.as_str();
        for line in text.lines() {
            let Some(value) = line.strip_prefix("    uses: ") else {
                continue;
            };
            let uses = unquote(value.trim())
                .split_once(" #")
                .map_or(unquote(value.trim()), |(uses, _)| uses);
            if !looks_like_reusable_workflow(uses) {
                continue;
            }
            if !is_local_reusable_workflow(uses) {
                return Err(invalid(format!(
                    "unsupported_external_reusable_workflow:{}:{uses}",
                    file.path
                )));
            }
            let target = uses
                .strip_prefix("./")
                .map(|target| target.split_once('@').map_or(target, |(path, _)| path))
                .ok_or_else(|| {
                    invalid(format!(
                        "invalid_local_reusable_workflow:{}:{uses}",
                        file.path
                    ))
                })?;
            if target.contains("..") || !workflows.contains(target) {
                return Err(invalid(format!(
                    "unresolved_local_reusable_workflow:{}:{uses}",
                    file.path
                )));
            }
        }
    }
    Ok(())
}

fn matrix_runner_labels(
    fields: &[(String, Yaml)],
    key: &str,
    path: &str,
    job_id: &str,
) -> Result<Vec<String>, RenderError> {
    let matrix = fields
        .iter()
        .find(|(name, _)| name == "strategy")
        .and_then(|(_, value)| match value {
            Yaml::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == "matrix")
                .map(|(_, value)| value),
            _ => None,
        })
        .and_then(|value| match value {
            Yaml::Map(entries) => Some(entries.as_slice()),
            _ => None,
        })
        .ok_or_else(|| {
            invalid(format!(
                "matrix_runner_values_missing:{path}:{job_id}:{key}"
            ))
        })?;
    let mut labels = Vec::new();
    let runner_axis = matrix
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value);
    if matrix.iter().any(|(name, _)| name == "exclude") {
        return Err(invalid(format!(
            "matrix_runner_exclusions_unsupported:{path}:{job_id}:{key}"
        )));
    }
    if let Some(axis) = runner_axis {
        labels.extend(matrix_axis_values(axis, path, job_id, key)?);
    }
    if let Some((_, include)) = matrix.iter().find(|(name, _)| name == "include") {
        let Yaml::Seq(include) = include else {
            return Err(invalid(format!(
                "matrix_runner_include_invalid:{path}:{job_id}:{key}"
            )));
        };
        for row in include {
            let Yaml::Map(entries) = row else {
                return Err(invalid(format!(
                    "matrix_runner_include_invalid:{path}:{job_id}:{key}"
                )));
            };
            if let Some((_, value)) = entries.iter().find(|(name, _)| name == key) {
                labels.push(matrix_include_runner(value, path, job_id, key)?);
            } else {
                return Err(invalid(format!(
                    "matrix_runner_value_missing:{path}:{job_id}:{key}"
                )));
            }
        }
    }
    if labels.is_empty() {
        return Err(invalid(format!(
            "matrix_runner_values_missing:{path}:{job_id}:{key}"
        )));
    }
    Ok(labels)
}

fn matrix_axis_values(
    value: &Yaml,
    path: &str,
    job_id: &str,
    key: &str,
) -> Result<Vec<String>, RenderError> {
    let values: Vec<&Yaml> = match value {
        Yaml::Seq(values) => values.iter().collect(),
        Yaml::Flow(values) => return Ok(values.clone()),
        _ => {
            return Err(invalid(format!(
                "matrix_runner_value_invalid:{path}:{job_id}:{key}"
            )));
        }
    };
    values
        .into_iter()
        .map(|value| match value {
            Yaml::Str(value) | Yaml::Quoted(value) => Ok(value.clone()),
            _ => Err(invalid(format!(
                "matrix_runner_value_invalid:{path}:{job_id}:{key}"
            ))),
        })
        .collect()
}

fn matrix_include_runner(
    value: &Yaml,
    path: &str,
    job_id: &str,
    key: &str,
) -> Result<String, RenderError> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) => Ok(value.clone()),
        _ => Err(invalid(format!(
            "matrix_runner_value_invalid:{path}:{job_id}:{key}"
        ))),
    }
}

fn parsed_hosted_label(value: &str, path: &str, job_id: &str) -> Result<String, RenderError> {
    match RunsOn::parse(value).map_err(|_| {
        invalid(format!(
            "unsupported_runs_on_for_linux_amd64_scale_set:{path}:{job_id}:{value}"
        ))
    })? {
        RunsOn::Hosted(label) => Ok(label),
        RunsOn::ScaleSet(_) => Err(invalid(format!(
            "unsupported_runs_on_for_linux_amd64_scale_set:{path}:{job_id}:{value}"
        ))),
    }
}

fn reject_unsupported_targets(
    labels: &[String],
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let mut parsed = Vec::with_capacity(labels.len());
    for label in labels {
        let platform = runner_platform(label);
        if platform == Some(RunnerPlatform::Darwin) {
            return Err(invalid(format!(
                "unsupported_darwin_workload:{path}:{job_id}:{label}"
            )));
        }
        parsed.push((label, platform));
    }
    for (label, platform) in parsed {
        if platform != Some(RunnerPlatform::LinuxAmd64) {
            return Err(invalid(format!(
                "unsupported_workload_for_linux_amd64_scale_set:{path}:{job_id}:{label}"
            )));
        }
    }
    Ok(())
}

fn matrix_expression_key(value: &str) -> Option<&str> {
    let key = value.strip_prefix("${{ matrix.")?.strip_suffix(" }}")?;
    (!key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    .then_some(key)
}

fn is_local_reusable_workflow(uses: &str) -> bool {
    uses.starts_with("./.github/workflows/")
}

fn looks_like_reusable_workflow(uses: &str) -> bool {
    uses.contains(".github/workflows/")
}

fn is_workflow_path(path: &str) -> bool {
    path.starts_with(".github/workflows/")
        && Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
            })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_tests.rs"]
mod tests;
