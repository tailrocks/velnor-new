use velnor_actions_contract::{ExecutionMode, RunsOn, ScaleSetSelector};

use crate::{RenderError, yaml::Yaml};

use super::{invalid, labels, matrix};

pub(super) fn route_runner_value(
    value: &Yaml,
    fields: &[(String, Yaml)],
    selector: &ScaleSetSelector,
    mode: Option<ExecutionMode>,
    path: &str,
    job_id: &str,
) -> Result<(bool, Option<Yaml>), RenderError> {
    match value {
        Yaml::Flow(labels) if labels == selector.labels() => Ok((true, None)),
        Yaml::Flow(labels) if mode != Some(ExecutionMode::ScaleSet) => {
            super::labels::validate_preserved_hosted_labels(labels, path, job_id)?;
            Ok((false, None))
        }
        Yaml::Flow(_) => Err(invalid(format!(
            "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
        ))),
        Yaml::Seq(labels) => route_sequence_runner(labels, selector, mode, path, job_id),
        Yaml::Str(expression) if matrix::is_matrix_expression(expression) => {
            route_matrix_runner(fields, expression, selector, mode, path, job_id)
        }
        Yaml::Str(value) => route_static_runner(value, selector, mode, path, job_id),
        _ => Err(invalid(format!(
            "unsupported_runs_on_for_linux_amd64_scale_set:{path}:{job_id}"
        ))),
    }
}

fn route_sequence_runner(
    values: &[Yaml],
    selector: &ScaleSetSelector,
    mode: Option<ExecutionMode>,
    path: &str,
    job_id: &str,
) -> Result<(bool, Option<Yaml>), RenderError> {
    let labels = values
        .iter()
        .map(|label| match label {
            Yaml::Str(label) | Yaml::Quoted(label) => Ok(label.as_str()),
            _ => Err(invalid(format!(
                "workflow_runner_label_not_scalar:{path}:{job_id}"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if labels
        == selector
            .labels()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        Ok((true, None))
    } else if mode == Some(ExecutionMode::ScaleSet) {
        Err(invalid(format!(
            "workflow_job_not_routed_to_scale_set:{path}:{job_id}"
        )))
    } else {
        labels::validate_preserved_hosted_labels(
            &labels
                .iter()
                .map(|label| (*label).to_owned())
                .collect::<Vec<_>>(),
            path,
            job_id,
        )?;
        Ok((false, None))
    }
}

fn route_matrix_runner(
    fields: &[(String, Yaml)],
    expression: &str,
    selector: &ScaleSetSelector,
    mode: Option<ExecutionMode>,
    path: &str,
    job_id: &str,
) -> Result<(bool, Option<Yaml>), RenderError> {
    let labels = matrix::matrix_runner_labels(fields, expression, path, job_id)?;
    match mode {
        Some(ExecutionMode::ScaleSet) => {
            labels::reject_unsupported_targets(&labels, path, job_id)?;
            Ok((true, Some(Yaml::Flow(selector.labels().to_vec()))))
        }
        Some(ExecutionMode::Hosted | ExecutionMode::Both) => {
            route_preserved_matrix(&labels, selector, path, job_id)
        }
        None => Ok((false, None)),
    }
}

fn route_preserved_matrix(
    labels: &[String],
    selector: &ScaleSetSelector,
    path: &str,
    job_id: &str,
) -> Result<(bool, Option<Yaml>), RenderError> {
    let selected = labels
        .iter()
        .filter(|label| *label == &selector.token())
        .count();
    if selected != 0 && selected != labels.len() {
        return Err(invalid(format!(
            "matrix_runner_policy_mixed:{path}:{job_id}"
        )));
    }
    if selected == labels.len() {
        Ok((true, Some(Yaml::Flow(selector.labels().to_vec()))))
    } else {
        labels::validate_preserved_hosted_labels(labels, path, job_id)?;
        Ok((false, None))
    }
}

fn route_static_runner(
    value: &str,
    selector: &ScaleSetSelector,
    mode: Option<ExecutionMode>,
    path: &str,
    job_id: &str,
) -> Result<(bool, Option<Yaml>), RenderError> {
    let parsed = RunsOn::parse(value).map_err(|_| {
        invalid(format!(
            "unsupported_runs_on_for_linux_amd64_scale_set:{path}:{job_id}:{value}"
        ))
    })?;
    match parsed {
        RunsOn::ScaleSet(existing) if existing == *selector => {
            Ok((true, Some(Yaml::Flow(selector.labels().to_vec()))))
        }
        RunsOn::ScaleSet(_) => Err(invalid(format!(
            "scale_set_selector_mismatch:{path}:{job_id}"
        ))),
        RunsOn::Hosted(label) => match mode {
            Some(ExecutionMode::ScaleSet) => {
                labels::reject_unsupported_targets(std::slice::from_ref(&label), path, job_id)?;
                Ok((true, Some(Yaml::Flow(selector.labels().to_vec()))))
            }
            Some(ExecutionMode::Hosted | ExecutionMode::Both) => {
                labels::validate_preserved_hosted_labels(
                    std::slice::from_ref(&label),
                    path,
                    job_id,
                )?;
                Ok((false, None))
            }
            None => Ok((false, None)),
        },
    }
}
