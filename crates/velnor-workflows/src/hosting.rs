// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::looks_github_hosted;

use crate::check::{Context, runner_label_of};
use crate::error::{WorkflowsError, refuse};
use crate::matrix_runner::MatrixRunner;
use crate::matrix_runner::matrix_runner;
use crate::plan::Matrix;
use crate::raw::RawJob;

/// Whether `label` is open to pull requests from forks. Only `hosted = true` opens it.
/// An undeclared label is self-hosted and fenced, except that one spelled like a
/// GitHub-hosted label is refused, so the repository has to say which it is.
pub(crate) fn resolve(
    owner: &str,
    label: &str,
    declared: Option<bool>,
) -> Result<bool, WorkflowsError> {
    match declared {
        Some(hosted) => Ok(hosted),
        None if looks_github_hosted(label) => refuse(format!(
            "runner {label} of {owner} looks GitHub-hosted; declare `hosted = true` (open to fork pull requests) or `hosted = false` (fenced)"
        )),
        None => Ok(false),
    }
}

/// Where one job runs: a fixed label, or the axis of its matrix.
pub(crate) struct Chosen {
    pub(crate) runner: Option<String>,
    pub(crate) matrix_runner: Option<MatrixRunner>,
    pub(crate) hosted: bool,
}

/// The runner of one job and whether it is open to fork pull requests: its own `runner` and
/// `hosted`, the workflow's, or, for `runner = "${{ matrix.<axis> }}"`, `runner_labels`.
pub(crate) fn of_job(
    ctx: &Context<'_>,
    id: &str,
    raw: &RawJob,
    matrix: Option<&Matrix>,
) -> Result<Chosen, WorkflowsError> {
    if let Some(expression) = raw
        .runner
        .as_deref()
        .filter(|runner| runner.contains(['$', '{']))
    {
        let runner = matrix_runner(id, expression, raw, matrix)?;
        let hosted = runner.labels.iter().all(|(_, hosted)| *hosted);
        return Ok(Chosen {
            runner: None,
            matrix_runner: Some(runner),
            hosted,
        });
    }
    if !raw.runner_labels.is_empty() {
        return refuse(format!(
            "job {id} sets runner_labels without a runner of the form ${{{{ matrix.<axis> }}}}"
        ));
    }
    let runner = raw
        .runner
        .as_deref()
        .map(|label| runner_label_of(ctx.file, label))
        .transpose()?;
    let hosted = fixed(ctx, id, runner.as_deref(), raw.hosted)?;
    Ok(Chosen {
        runner,
        matrix_runner: None,
        hosted,
    })
}

fn fixed(
    ctx: &Context<'_>,
    id: &str,
    runner: Option<&str>,
    declared: Option<bool>,
) -> Result<bool, WorkflowsError> {
    match runner {
        None if declared.is_some() => refuse(format!("job {id} sets hosted without runner")),
        None => Ok(ctx.hosted),
        Some(label) if declared.is_none() && label.eq_ignore_ascii_case(ctx.runner) => {
            Ok(ctx.hosted)
        }
        Some(label) => resolve(&format!("job {id}"), label, declared),
    }
}
