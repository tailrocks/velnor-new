// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{Job, Matrix, MatrixSource};
use crate::raw::RawMatrix;

const MAX_COMBINATIONS: usize = 256;

pub(crate) fn matrix(
    id: &str,
    raw: RawMatrix,
    needs: &[String],
    earlier: &[Job],
) -> Result<Matrix, WorkflowsError> {
    if raw.max_parallel == Some(0) {
        return refuse(format!("job {id} matrix max_parallel must be at least 1"));
    }
    let source = match (raw.from_output, raw.axes) {
        (Some(from), None) => from_output(id, &from, needs, earlier)?,
        (None, Some(axes)) => axes_of(id, axes)?,
        _ => {
            return refuse(format!(
                "job {id} matrix needs exactly one of from_output and axes"
            ));
        }
    };
    Ok(Matrix {
        source,
        fail_fast: raw.fail_fast,
        max_parallel: raw.max_parallel,
    })
}

pub(crate) fn axis_names(matrix: &Matrix) -> Option<Vec<String>> {
    match &matrix.source {
        MatrixSource::Axes(axes) => Some(axes.iter().map(|(name, _)| name.clone()).collect()),
        MatrixSource::Output { .. } => None,
    }
}

fn from_output(
    id: &str,
    from: &str,
    needs: &[String],
    earlier: &[Job],
) -> Result<MatrixSource, WorkflowsError> {
    let Some((from_job, from_output)) = from.split_once('.') else {
        return refuse(format!(
            "job {id} matrix from_output must be <job>.<output>"
        ));
    };
    if !needs.iter().any(|need| need == from_job) {
        return refuse(format!(
            "job {id} builds its matrix from the output of {from_job}, so it must list {from_job} in needs"
        ));
    }
    let declared = earlier
        .iter()
        .find(|seen| seen.id == from_job)
        .is_some_and(|seen| seen.output_names().any(|name| name == from_output));
    if !declared {
        return refuse(format!(
            "job {id} builds its matrix from {from_job}.{from_output}, which {from_job} does not declare as an output"
        ));
    }
    Ok(MatrixSource::Output {
        from_job: from_job.to_owned(),
        from_output: from_output.to_owned(),
    })
}

fn axes_of(
    id: &str,
    axes: std::collections::BTreeMap<String, Vec<String>>,
) -> Result<MatrixSource, WorkflowsError> {
    if axes.is_empty() {
        return refuse(format!("job {id} matrix axes must not be empty"));
    }
    let mut combinations: usize = 1;
    for (name, values) in &axes {
        let key_ok = is_identifier(name, true) && !matches!(name.as_str(), "include" | "exclude");
        let values_ok = !values.is_empty()
            && values.iter().enumerate().all(|(index, value)| {
                !value.is_empty()
                    && !value.contains(char::is_control)
                    && !value.contains("${{")
                    && !values[..index].contains(value)
            });
        if !key_ok || !values_ok {
            return refuse(format!(
                "job {id} matrix axis {name:?} must be an identifier (not include or exclude) with unique, non-empty values without expressions"
            ));
        }
        combinations = combinations.saturating_mul(values.len());
    }
    if combinations > MAX_COMBINATIONS {
        return refuse(format!(
            "job {id} matrix has {combinations} combinations, and GitHub allows {MAX_COMBINATIONS}"
        ));
    }
    Ok(MatrixSource::Axes(axes.into_iter().collect()))
}
