// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::looks_github_hosted;

use crate::check::Context;
use crate::error::{WorkflowsError, refuse};

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

/// The hosting of the runner of one job: its own `runner` and `hosted`, or the workflow's.
pub(crate) fn of_job(
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
