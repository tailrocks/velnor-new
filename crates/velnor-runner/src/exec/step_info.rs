// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::output::Output;
use super::token;
use super::{AnnotationLevel, ExecError};
use crate::expr::{ExprEnv, evaluate_template};
use crate::job_message::ActionStep;

use super::preflight::reference;

const TIMEOUT_FAILED: &str = "An error occurred when attempting to determine the step timeout.";
const CONTINUE_ON_ERROR_FAILED: &str = "The step failed and an error occurred when attempting to determine whether to continue on error.";

pub(super) fn context_id(step: &ActionStep) -> Option<&str> {
    step.context_name
        .as_deref()
        .filter(|name| !name.is_empty() && !name.starts_with("__"))
}

pub(super) fn step_name(step: &ActionStep, env: &ExprEnv) -> String {
    step.display_name_template()
        .and_then(|template| evaluate_template(&template, env).ok())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            step.script_text()
                .and_then(|script| script.lines().next())
                .map(|line| format!("Run {line}"))
        })
        .or_else(|| reference(step))
        .or_else(|| step.name.clone())
        .unwrap_or_else(|| "step".to_owned())
}

pub(super) fn step_environment(
    step: &ActionStep,
    env: &ExprEnv,
) -> Result<Vec<(String, String)>, ExecError> {
    match step.environment.as_ref() {
        Some(environment) => Ok(token::mapping(environment, env)?),
        None => Ok(Vec::new()),
    }
}

fn report_pair(out: &mut Output<'_>, headline: &str, detail: &str) {
    for line in [headline, detail] {
        out.message(line);
        out.annotate(AnnotationLevel::Error, line);
    }
}

pub(super) fn step_timeout(step: &ActionStep, env: &ExprEnv, out: &mut Output<'_>) -> Option<f64> {
    let token = step.timeout_in_minutes.as_ref()?;
    let text = match token::scalar(token, env) {
        Ok(text) => text,
        Err(error) => {
            report_pair(out, TIMEOUT_FAILED, &error.to_string());
            return None;
        }
    };
    match text.trim().parse::<f64>() {
        Ok(minutes) if minutes.is_finite() => Some(minutes).filter(|minutes| *minutes > 0.0),
        _ => {
            if !text.trim().is_empty() {
                report_pair(out, TIMEOUT_FAILED, &format!("'{text}' is not a number"));
            }
            None
        }
    }
}

pub(super) fn continue_on_error(step: &ActionStep, env: &ExprEnv, out: &mut Output<'_>) -> bool {
    let Some(token) = step.continue_on_error.as_ref() else {
        return false;
    };
    match token::boolean(token, env) {
        Ok(value) => value,
        Err(error) => {
            report_pair(out, CONTINUE_ON_ERROR_FAILED, &error.to_string());
            false
        }
    }
}
