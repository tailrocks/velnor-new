// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::plan::{Matrix, MatrixRunner, MatrixSource};
use crate::raw::RawJob;

/// `runner = "${{ matrix.<axis> }}"`. Every value of the axis must be declared in
/// `runner_labels`, because the generator never decides from a label whether a runner is
/// hosted. A pull request from a fork then reaches the job unfenced only when every label
/// is `true`.
pub(crate) fn matrix_runner(
    id: &str,
    expression: &str,
    raw: &RawJob,
    matrix: Option<&Matrix>,
) -> Result<MatrixRunner, WorkflowsError> {
    let Some(axis) = axis_of(expression) else {
        return refuse(format!(
            "job {id} runner {expression:?} must be a label or exactly ${{{{ matrix.<axis> }}}} of the job matrix"
        ));
    };
    if raw.hosted.is_some() {
        return refuse(format!(
            "job {id} runner reads matrix.{axis}, so declare each label in runner_labels instead of hosted"
        ));
    }
    let values = match matrix.map(|matrix| &matrix.source) {
        Some(MatrixSource::Axes { axes, include }) => axis_values(axis, axes, include),
        Some(MatrixSource::Output { .. }) => {
            return refuse(format!(
                "job {id} runner reads matrix.{axis}, which needs a matrix of static axes or include entries, not from_output, so that every label can be declared"
            ));
        }
        None => Vec::new(),
    };
    if values.is_empty() {
        return refuse(format!(
            "job {id} runner reads matrix.{axis}, which is not an axis or include key of the job matrix"
        ));
    }
    let mut labels = Vec::new();
    for value in &values {
        let hosted = raw.runner_labels.get(*value).copied();
        let Some(hosted) = hosted else {
            return refuse(format!(
                "job {id} runs on matrix.{axis} = {value:?}, which is not declared in runner_labels; declare it `true` (GitHub-hosted) or `false` (self-hosted)"
            ));
        };
        labels.push((runner_label_of(&format!("job {id}"), value)?, hosted));
    }
    if let Some(unused) = raw
        .runner_labels
        .keys()
        .find(|label| !values.contains(&label.as_str()))
    {
        return refuse(format!(
            "job {id} declares runner_labels {unused:?}, which no value of matrix.{axis} uses"
        ));
    }
    Ok(MatrixRunner {
        axis: axis.to_owned(),
        labels,
    })
}

fn axis_of(expression: &str) -> Option<&str> {
    let inner = expression.strip_prefix("${{")?.strip_suffix("}}")?.trim();
    let axis = inner.strip_prefix("matrix.")?;
    crate::expression::is_identifier(axis, true).then_some(axis)
}

fn axis_values<'a>(
    axis: &str,
    axes: &'a [(String, Vec<String>)],
    include: &'a [Vec<(String, String)>],
) -> Vec<&'a str> {
    let mut values: Vec<&str> = axes
        .iter()
        .filter(|(name, _)| name == axis)
        .flat_map(|(_, values)| values.iter().map(String::as_str))
        .collect();
    for (_, value) in include.iter().flatten().filter(|(name, _)| name == axis) {
        if !values.contains(&value.as_str()) {
            values.push(value);
        }
    }
    values
}
