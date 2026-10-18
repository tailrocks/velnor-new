//! Shared placement policy for every generated workflow document.

use velnor_actions_contract::{
    ExecutionMode, Job, RunnerPlatform, RunsOn, ScaleSetSelector, runner_platform,
};

use crate::{RenderError, RenderedFile, yaml::Yaml};

const MAX_MATRIX_COMBINATIONS: usize = 256;

#[path = "workflow_routing_inventory.rs"]
mod inventory;
#[path = "workflow_routing_matrix.rs"]
mod matrix;
#[path = "workflow_routing_parse.rs"]
mod parse;

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
            Yaml::Str(value) if matrix::is_matrix_expression(value) => {
                let labels = matrix::matrix_runner_labels(fields, value, path, job_id)?;
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

/// Apply the selected runner placement after policy jobs have been merged.
///
/// Keeping this on the typed job map covers support jobs added after the
/// user's workflow IR, including repository lint jobs.
pub(crate) fn route_jobs(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    let WorkflowRoutingPolicy::ScaleSet(selector) = policy else {
        return Ok(());
    };
    for (job_id, job) in jobs {
        match RunsOn::parse(&job.runs_on).map_err(|_| {
            invalid(format!(
                "unsupported_runs_on_for_linux_amd64_scale_set:.github/workflows/ci.yml:{job_id}:{}",
                job.runs_on
            ))
        })? {
            RunsOn::ScaleSet(existing) if existing == *selector => {}
            RunsOn::ScaleSet(_) => {
                return Err(invalid(format!(
                    "scale_set_selector_mismatch:.github/workflows/ci.yml:{job_id}"
                )));
            }
            RunsOn::Hosted(label) => match runner_platform(&label) {
                Some(RunnerPlatform::Darwin) => {
                    return Err(invalid(format!(
                        "unsupported_darwin_workload:.github/workflows/ci.yml:{job_id}:{label}"
                    )));
                }
                Some(RunnerPlatform::LinuxAmd64) => job.runs_on = selector.token(),
                Some(RunnerPlatform::LinuxArm64) | None => {
                    return Err(invalid(format!(
                        "unsupported_workload_for_linux_amd64_scale_set:.github/workflows/ci.yml:{job_id}:{label}"
                    )));
                }
            },
        }
    }
    Ok(())
}

/// Examine every job runner and reusable-workflow call in the final workflow files.
///
/// # Errors
///
/// The function gives an error for incorrect workflow data, unsupported Scale Set placement,
/// or a reusable-workflow call whose target is missing, does not accept calls, or creates a
/// call cycle.
pub fn validate_generated_workflow_calls(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    inventory::validate_final_workflow_inventory(files, policy)
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

fn is_local_reusable_workflow(uses: &str) -> bool {
    uses.starts_with("./.github/workflows/")
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_tests.rs"]
mod tests;
