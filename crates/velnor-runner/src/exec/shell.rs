// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/container.rs

use std::path::Path;

use super::context::JobContext;
use super::{ExecError, token};
use crate::expr::ExprEnv;
use crate::job_message::ActionStep;

pub(super) const DEFAULT_SHELL: &str = "bash -e {0}";

const PLACEHOLDER: &str = "{0}";

fn expand(spec: &str) -> &str {
    match spec.trim() {
        "bash" => "bash --noprofile --norc -e -o pipefail {0}",
        "sh" => "sh -e {0}",
        other => other,
    }
}

pub(super) fn is_supported(spec: &str) -> bool {
    matches!(expand(spec).split_whitespace().next(), Some("bash" | "sh"))
}

pub(super) fn command(spec: &str, script: &Path) -> Option<(String, Vec<String>)> {
    let script = script.to_string_lossy();
    let mut tokens = expand(spec).split_whitespace();
    let program = tokens.next()?.to_owned();
    let mut args: Vec<String> = tokens
        .map(|token| token.replace(PLACEHOLDER, &script))
        .collect();
    if !expand(spec).contains(PLACEHOLDER) {
        args.push(script.into_owned());
    }
    Some((program, args))
}

pub(super) fn resolve(
    step: &ActionStep,
    context: &JobContext,
    env: &ExprEnv,
) -> Result<String, ExecError> {
    let chosen =
        token::input(step.inputs.as_ref(), "shell", env)?.filter(|shell| !shell.trim().is_empty());
    if let Some(shell) = chosen {
        return Ok(shell);
    }
    Ok(context
        .default_run_value("shell", env)?
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SHELL.to_owned()))
}
