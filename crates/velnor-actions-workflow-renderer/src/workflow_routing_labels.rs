use velnor_actions_contract::config::is_hosted_catalog;
use velnor_actions_contract::{ReleaseTarget, RunnerPlatform, RunsOn, runner_platform};

use crate::{RenderError, yaml::Yaml};

pub(super) fn reject_duplicate_keys(
    fields: &[(String, crate::yaml::Yaml)],
    path: &str,
    owner: &str,
) -> Result<(), RenderError> {
    let mut seen = std::collections::BTreeSet::new();
    for (key, _) in fields {
        if !seen.insert(key.as_str()) {
            return Err(super::invalid(format!(
                "workflow_field_duplicate:{path}:{owner}:{key}"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_preserved_hosted_labels(
    labels: &[String],
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    if labels
        .iter()
        .all(|label| is_hosted_catalog(label) || ReleaseTarget::for_runner_label(label).is_some())
    {
        Ok(())
    } else {
        Err(super::invalid(format!(
            "unsupported_hosted_runner:{path}:{job_id}"
        )))
    }
}

pub(super) fn reject_unsupported_targets(
    labels: &[String],
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let mut parsed = Vec::with_capacity(labels.len());
    for label in labels {
        let platform = runner_platform(label);
        if platform == Some(RunnerPlatform::Darwin) {
            return Err(super::invalid(format!(
                "unsupported_darwin_workload:{path}:{job_id}:{label}"
            )));
        }
        parsed.push((label, platform));
    }
    for (label, platform) in parsed {
        if platform != Some(RunnerPlatform::LinuxAmd64) {
            return Err(super::invalid(format!(
                "unsupported_workload_for_linux_amd64_scale_set:{path}:{job_id}:{label}"
            )));
        }
    }
    Ok(())
}

pub(super) fn is_local_reusable_workflow(uses: &str) -> bool {
    uses.starts_with("./.github/workflows/")
}

pub(super) fn validate_hosted_runner(
    value: &Yaml,
    fields: &[(String, Yaml)],
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let labels = match value {
        Yaml::Flow(values) => values.clone(),
        Yaml::Seq(values) => values
            .iter()
            .map(|value| match value {
                Yaml::Str(value) | Yaml::Quoted(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| unsupported_runner(path, job_id))?,
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => {
            if super::matrix::is_matrix_expression(value) {
                super::matrix::matrix_runner_labels(fields, value, path, job_id)?
            } else {
                match RunsOn::parse(value) {
                    Ok(RunsOn::Hosted(label)) => vec![label],
                    Ok(RunsOn::ScaleSet(_)) | Err(_) => {
                        return Err(unsupported_runner(path, job_id));
                    }
                }
            }
        }
        _ => return Err(unsupported_runner(path, job_id)),
    };
    validate_preserved_hosted_labels(&labels, path, job_id)
}

fn unsupported_runner(path: &str, job_id: &str) -> RenderError {
    super::invalid(format!("unsupported_hosted_runner:{path}:{job_id}"))
}
