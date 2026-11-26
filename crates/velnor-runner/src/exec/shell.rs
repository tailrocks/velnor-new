// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/container.rs

use std::path::Path;

use serde_json::Value as Json;

use super::context::JobContext;
use super::{ExecError, token};
use crate::expr::ExprEnv;
use crate::job_message::ActionStep;
use crate::job_message::template_token::token_literal;

pub(super) const DEFAULT_SHELL: &str = "bash -e {0}";

const PLACEHOLDER: &str = "{0}";

fn expand(spec: &str) -> &str {
    match spec.trim() {
        "bash" => "bash --noprofile --norc -e -o pipefail {0}",
        "sh" => "sh -e {0}",
        other => other,
    }
}

fn split(spec: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = spec.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                let mut backslashes = 1;
                while chars.next_if_eq(&'\\').is_some() {
                    backslashes += 1;
                }
                started = true;
                if chars.peek() == Some(&'"') {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        current.push('"');
                        chars.next();
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                }
            }
            '"' => {
                started = true;
                if quoted && chars.next_if_eq(&'"').is_some() {
                    current.push('"');
                } else {
                    quoted = !quoted;
                }
            }
            ' ' | '\t' if !quoted => {
                if std::mem::take(&mut started) {
                    arguments.push(std::mem::take(&mut current));
                }
            }
            other => {
                started = true;
                current.push(other);
            }
        }
    }
    if started {
        arguments.push(current);
    }
    arguments
}

pub(super) fn is_supported(spec: &str) -> bool {
    let expanded = expand(spec);
    expanded.contains(PLACEHOLDER)
        && matches!(
            split(expanded).first().map(String::as_str),
            Some("bash" | "sh")
        )
}

pub(super) fn command(spec: &str, script: &Path) -> Option<(String, Vec<String>)> {
    let script = script.to_string_lossy();
    let expanded = expand(spec);
    let mut tokens = split(expanded).into_iter();
    let program = tokens.next()?;
    let args = tokens
        .map(|token| token.replace(PLACEHOLDER, &script))
        .collect();
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
        .default_run("shell")
        .filter(|shell| !shell.trim().is_empty())
        .map_or_else(|| DEFAULT_SHELL.to_owned(), str::to_owned))
}

fn plain(token: &Json) -> Option<&str> {
    token_literal(token).filter(|text| !text.contains("${{") && !text.trim().is_empty())
}

pub(super) fn admission_spec(step: &ActionStep, context: &JobContext) -> Option<String> {
    let own = step
        .inputs
        .as_ref()
        .and_then(|inputs| token::find(inputs, "shell"));
    if let Some(token) = own {
        match token_literal(token) {
            Some(text) if text.trim().is_empty() => {}
            _ => return plain(token).map(str::to_owned),
        }
    }
    context
        .default_run("shell")
        .filter(|shell| !shell.trim().is_empty())
        .map(str::to_owned)
}
