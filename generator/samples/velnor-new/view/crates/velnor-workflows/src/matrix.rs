// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

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
    let source = match (raw.from_output, raw.axes, raw.include) {
        (Some(from), None, include) if include.is_empty() => {
            from_output(id, &from, needs, earlier)?
        }
        (None, axes, include) if axes.is_some() || !include.is_empty() => {
            axes_of(id, axes.unwrap_or_default(), include)?
        }
        _ => {
            return refuse(format!(
                "job {id} matrix needs exactly one source: from_output alone, or axes and include (at least one of them)"
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
        MatrixSource::Axes { axes, include } => {
            let mut names: Vec<String> = axes.iter().map(|(name, _)| name.clone()).collect();
            for (name, _) in include.iter().flatten() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            Some(names)
        }
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
    axes: BTreeMap<String, Vec<String>>,
    include: Vec<BTreeMap<String, String>>,
) -> Result<MatrixSource, WorkflowsError> {
    if axes.is_empty() && include.is_empty() {
        return refuse(format!("job {id} matrix axes must not be empty"));
    }
    let mut combinations: usize = usize::from(!axes.is_empty());
    for (index, (name, values)) in axes.iter().enumerate() {
        if axes
            .keys()
            .take(index)
            .any(|earlier| earlier.eq_ignore_ascii_case(name))
        {
            return refuse(format!(
                "job {id} matrix axis {name:?} differs from another axis only by case; GitHub reads matrix keys without regard to case"
            ));
        }
        let key_ok = matrix_key(name);
        let values_ok = !values.is_empty()
            && values
                .iter()
                .enumerate()
                .all(|(index, value)| plain_value(value) && !values[..index].contains(value));
        if !key_ok || !values_ok {
            return refuse(format!(
                "job {id} matrix axis {name:?} must be an identifier (not include or exclude) with unique, non-empty values without expressions"
            ));
        }
        combinations = combinations.saturating_mul(values.len());
    }
    let include = include_entries(id, &axes, include)?;
    combinations = combinations.saturating_add(include.len());
    if combinations > MAX_COMBINATIONS {
        return refuse(format!(
            "job {id} matrix has {combinations} combinations, and GitHub allows {MAX_COMBINATIONS}"
        ));
    }
    Ok(MatrixSource::Axes {
        axes: axes.into_iter().collect(),
        include,
    })
}

fn matrix_key(name: &str) -> bool {
    is_identifier(name, true) && !matches!(name, "include" | "exclude")
}

fn plain_value(value: &str) -> bool {
    !value.is_empty() && !value.contains(char::is_control) && !value.contains("${{")
}

fn include_entries(
    id: &str,
    axes: &BTreeMap<String, Vec<String>>,
    include: Vec<BTreeMap<String, String>>,
) -> Result<Vec<Vec<(String, String)>>, WorkflowsError> {
    let mut known: Vec<&str> = axes.keys().map(String::as_str).collect();
    for (index, entry) in include.iter().enumerate() {
        let bad_key = entry.keys().find(|key| {
            !matrix_key(key)
                || known
                    .iter()
                    .any(|seen| seen.eq_ignore_ascii_case(key) && seen != key)
        });
        if let Some(key) = bad_key {
            return refuse(format!(
                "job {id} matrix include entry {} has the key {key:?}, which must be an identifier (not include or exclude) spelled like an axis or earlier include key of the same name",
                index + 1
            ));
        }
        if entry.is_empty() || !entry.values().all(|value| plain_value(value)) {
            return refuse(format!(
                "job {id} matrix include entry {} must set at least one key, each to a non-empty value without expressions",
                index + 1
            ));
        }
        if include[..index].contains(entry) {
            return refuse(format!(
                "job {id} matrix include entry {} repeats an earlier entry",
                index + 1
            ));
        }
        if axes.is_empty() && include[0].keys().ne(entry.keys()) {
            return refuse(format!(
                "job {id} matrix include entry {} has other keys than the first entry; without axes every entry must set the same keys, or matrix.<key> would be empty in some jobs",
                index + 1
            ));
        }
        known.extend(entry.keys().map(String::as_str));
    }
    Ok(include
        .into_iter()
        .map(|entry| entry.into_iter().collect())
        .collect())
}
