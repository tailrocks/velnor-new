//! Provider-specific artifact task matrices and plan output wiring.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::config::RunsOn;
use velnor_actions_contract_workflow::{
    ARTIFACT_HOSTED_MATRIX_OUTPUT, ARTIFACT_MATRIX_MAX_PARALLEL_ENV, ARTIFACT_MATRIX_NEEDS_JOB_ENV,
    ARTIFACT_MATRIX_PROVIDER_ENV, ARTIFACT_VELNOR_MATRIX_OUTPUT, ArtifactBuildProvider,
    HOSTED_SUFFIX, Job, SCALE_SUFFIX, StepKind,
};
use velnor_actions_workflow_jobs::context::{PLAN_JOB_ID, TASK_JOB_ID};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

use crate::matrix::{has_step_id, insert_job_key, matrix_invalid, require_plan_step_id};

/// One validated artifact matrix consumer in the expanded workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMatrixDirective {
    /// Expanded job key receiving a provider matrix.
    pub job_id: String,
    /// Provider fixed by its expanded `runs-on` target.
    pub provider: ArtifactBuildProvider,
    /// Maximum concurrent jobs in this provider's build matrix.
    pub max_parallel: u32,
}

/// Discover artifact matrix markers after schema-2 lane expansion.
///
/// The provider is derived from the typed job runner, never a workflow
/// expression or caller-authored output name. The producer must be the
/// plan job and the consuming job must depend on it.
/// # Errors
pub fn artifact_matrix_directives(
    jobs: &BTreeMap<String, Job>,
) -> Result<Vec<ArtifactMatrixDirective>, RenderError> {
    let mut directives = Vec::new();
    let mut providers = BTreeSet::new();
    for (job_id, job) in jobs {
        let Some(markers) = artifact_markers(job)? else {
            continue;
        };
        let [Some(producer), Some(provider_marker), Some(max_parallel)] = markers.as_slice() else {
            return Err(matrix_invalid("artifact_matrix_marker_partial"));
        };
        if producer != PLAN_JOB_ID || provider_marker != "provider" {
            return Err(matrix_invalid("artifact_matrix_bad_producer_or_provider"));
        }
        if !job.needs.iter().any(|need| need == producer) {
            return Err(matrix_invalid("artifact_matrix_without_plan_need"));
        }
        let max_parallel = max_parallel
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| matrix_invalid("artifact_matrix_bad_max_parallel"))?;
        let selector = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
        let provider = if selector.is_scale_set() {
            ArtifactBuildProvider::VelnorScaleSet
        } else {
            ArtifactBuildProvider::GithubHosted
        };
        if (job_id.ends_with(HOSTED_SUFFIX) && provider != ArtifactBuildProvider::GithubHosted)
            || (job_id.ends_with(SCALE_SUFFIX) && provider != ArtifactBuildProvider::VelnorScaleSet)
        {
            return Err(matrix_invalid("artifact_matrix_lane_provider_mismatch"));
        }
        if job_id == TASK_JOB_ID {
            return Err(matrix_invalid("artifact_matrix_task_job_reserved"));
        }
        if !providers.insert(provider) {
            return Err(matrix_invalid("artifact_matrix_duplicate_provider_job"));
        }
        directives.push(ArtifactMatrixDirective {
            job_id: job_id.clone(),
            provider,
            max_parallel,
        });
    }
    Ok(directives)
}

/// Remove renderer-only artifact matrix markers before normal step validation.
#[must_use]
pub fn scrub_artifact_matrix_markers(jobs: &BTreeMap<String, Job>) -> BTreeMap<String, Job> {
    let mut scrubbed = jobs.clone();
    for job in scrubbed.values_mut() {
        for step in &mut job.steps {
            if let StepKind::Shell { env, .. } = &mut step.kind {
                env.remove(ARTIFACT_MATRIX_NEEDS_JOB_ENV);
                env.remove(ARTIFACT_MATRIX_PROVIDER_ENV);
                env.remove(ARTIFACT_MATRIX_MAX_PARALLEL_ENV);
            }
        }
    }
    scrubbed
}

/// Emit provider matrices and promote their plan outputs.
///
/// The matrix is derived at runtime from the same validated plan artifact
/// used by `Required`. Each strategy disables fail-fast so both providers
/// produce an observable result.
/// # Errors
pub fn attach_artifact_matrices(
    document: &mut Yaml,
    directives: &[ArtifactMatrixDirective],
) -> Result<(), RenderError> {
    if directives.is_empty() {
        return Ok(());
    }
    let mut providers = BTreeSet::new();
    for directive in directives {
        if directive.max_parallel == 0 {
            return Err(matrix_invalid("artifact_matrix_bad_max_parallel"));
        }
        if !providers.insert(directive.provider) {
            return Err(matrix_invalid("artifact_matrix_duplicate_provider_job"));
        }
    }
    let Yaml::Map(entries) = document else {
        return Err(matrix_invalid("matrix_without_document"));
    };
    let Some(Yaml::Map(jobs)) = entries
        .iter_mut()
        .find(|entry| entry.0 == "jobs")
        .map(|entry| &mut entry.1)
    else {
        return Err(matrix_invalid("matrix_without_jobs"));
    };
    let plan_output_pairs = directives
        .iter()
        .map(|directive| {
            let output = matrix_output(directive.provider);
            (
                output,
                Yaml::str(format!("${{{{ steps.plan.outputs.{output} }}}}")),
            )
        })
        .collect::<Vec<_>>();
    append_plan_outputs(jobs, &plan_output_pairs)?;
    for directive in directives {
        let output = matrix_output(directive.provider);
        replace_job_key(
            jobs,
            &directive.job_id,
            "name",
            Yaml::str("${{ format('Build artifact {0} / {1}', matrix.provider, matrix.task_id) }}"),
        )?;
        replace_job_key(
            jobs,
            &directive.job_id,
            "timeout-minutes",
            Yaml::str("${{ matrix.timeout_minutes }}"),
        )?;
        insert_job_key(
            jobs,
            &directive.job_id,
            "strategy",
            Yaml::Map(vec![
                ("fail-fast".to_owned(), Yaml::Bool(false)),
                (
                    "max-parallel".to_owned(),
                    Yaml::Int(directive.max_parallel.into()),
                ),
                (
                    "matrix".to_owned(),
                    Yaml::str(format!(
                        "${{{{ fromJSON(needs.{PLAN_JOB_ID}.outputs.{output}) }}}}"
                    )),
                ),
            ]),
        )?;
        require_plan_step_id(jobs, PLAN_JOB_ID, output)?;
    }
    Ok(())
}

fn artifact_markers(job: &Job) -> Result<Option<[Option<String>; 3]>, RenderError> {
    let mut found = None;
    for step in &job.steps {
        if let StepKind::Shell { env, .. } = &step.kind {
            let values = [
                ARTIFACT_MATRIX_NEEDS_JOB_ENV,
                ARTIFACT_MATRIX_PROVIDER_ENV,
                ARTIFACT_MATRIX_MAX_PARALLEL_ENV,
            ]
            .map(|key| env.get(key).cloned());
            if values.iter().any(Option::is_some) {
                if found.is_some() {
                    return Err(matrix_invalid("artifact_matrix_multiple_marker_steps"));
                }
                found = Some(values);
            }
        }
    }
    if let Some(markers) = &found
        && markers.iter().any(Option::is_none)
    {
        return Err(matrix_invalid("artifact_matrix_marker_partial"));
    }
    Ok(found)
}

fn append_plan_outputs(
    jobs: &mut [(String, Yaml)],
    outputs: &[(&'static str, Yaml)],
) -> Result<(), RenderError> {
    if !has_step_id(jobs, PLAN_JOB_ID, "plan") {
        return Err(matrix_invalid("matrix_without_plan_step:artifact"));
    }
    let Some(Yaml::Map(plan)) = jobs
        .iter_mut()
        .find_map(|(id, job)| (id == PLAN_JOB_ID).then_some(job))
    else {
        return Err(matrix_invalid("matrix_without_plan_job"));
    };
    if let Some((_, value)) = plan.iter_mut().find(|(key, _)| key == "outputs") {
        let Yaml::Map(existing) = value else {
            return Err(matrix_invalid("artifact_matrix_bad_plan_outputs"));
        };
        let mut names: BTreeSet<String> = existing.iter().map(|(name, _)| name.clone()).collect();
        for (name, value) in outputs {
            if !names.insert((*name).to_owned()) {
                return Err(matrix_invalid("artifact_matrix_output_collision"));
            }
            existing.push(((*name).to_owned(), value.clone()));
        }
        return Ok(());
    }
    let mut values = Vec::with_capacity(outputs.len());
    for (name, value) in outputs {
        values.push(((*name).to_owned(), value.clone()));
    }
    insert_job_key(jobs, PLAN_JOB_ID, "outputs", Yaml::Map(values))
}

fn replace_job_key(
    jobs: &mut [(String, Yaml)],
    id: &str,
    key: &str,
    value: Yaml,
) -> Result<(), RenderError> {
    let Some(entries) = jobs.iter_mut().find_map(|(name, job)| {
        if name == id
            && let Yaml::Map(entries) = job
        {
            return Some(entries);
        }
        None
    }) else {
        return Err(matrix_invalid(&format!("artifact_matrix_without_job:{id}")));
    };
    let Some((_, existing)) = entries.iter_mut().find(|(name, _)| name == key) else {
        return Err(matrix_invalid(&format!("artifact_matrix_without_{key}")));
    };
    *existing = value;
    Ok(())
}

const fn matrix_output(provider: ArtifactBuildProvider) -> &'static str {
    match provider {
        ArtifactBuildProvider::GithubHosted => ARTIFACT_HOSTED_MATRIX_OUTPUT,
        ArtifactBuildProvider::VelnorScaleSet => ARTIFACT_VELNOR_MATRIX_OUTPUT,
    }
}

#[cfg(test)]
mod tests;
