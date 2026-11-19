// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};

/// Where an expression is written, which decides the contexts it may read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// A job `if`: no matrix, steps or job context.
    Job,
    /// A `with` of a job that calls a reusable workflow: the matrix, no steps.
    Call,
    Step,
}

pub(crate) struct Scope<'a> {
    pub(crate) inputs: &'a [&'a str],
    pub(crate) has_matrix: bool,
    /// The keys of a static matrix; `None` for a matrix built from an output.
    pub(crate) matrix_keys: Option<&'a [String]>,
    pub(crate) level: Level,
    pub(crate) needs: &'a [String],
    pub(crate) needed_outputs: &'a [(&'a str, &'a str)],
    pub(crate) steps: Vec<String>,
    /// The events of the workflow, as `Triggers::events` lists them.
    pub(crate) events: &'a [&'static str],
}

pub(crate) fn is_identifier(text: &str, dashes: bool) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || (dashes && ch == '-'))
}

pub(crate) fn whole_expression(value: &str) -> Option<&str> {
    let inner = value.strip_prefix("${{")?.strip_suffix("}}")?;
    (!inner.contains("${{") && !inner.contains("}}")).then_some(inner.trim())
}

pub(crate) fn bare(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map_or(text, str::trim)
}

fn references(expression: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let characters: Vec<char> = expression.chars().chain(std::iter::once(' ')).collect();
    for (index, &c) in characters.iter().enumerate() {
        if c == '\'' {
            quoted = !quoted;
        }
        if !quoted && c != '\'' && (c.is_ascii_alphanumeric() || "_.*-".contains(c)) {
            token.push(c);
            continue;
        }
        let calls = characters[index..]
            .iter()
            .find(|next| !next.is_whitespace())
            .is_some_and(|next| *next == '(' && !quoted);
        let literal = matches!(token.as_str(), "true" | "false" | "null");
        if token.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
            && !calls
            && !literal
        {
            found.push(token.clone());
        }
        token.clear();
    }
    found
}

fn allowed(reference: &str, scope: &Scope<'_>, condition: bool) -> bool {
    let parts: Vec<&str> = reference.split('.').collect();
    let part = |index: usize| parts.get(index).copied().unwrap_or("");
    match part(0) {
        "needs" if condition && part(1) == "*" => true,
        "needs" => {
            scope.needs.iter().any(|need| need == part(1))
                && match parts.as_slice() {
                    [_, _, "outputs", output] => scope.needed_outputs.contains(&(part(1), output)),
                    [_, _, "result"] => condition,
                    _ => false,
                }
        }
        "steps" if scope.level != Level::Step => false,
        "steps" => {
            scope.steps.iter().any(|step| step == part(1))
                && match parts.as_slice() {
                    [_, _, "outputs", output] => is_identifier(output, true),
                    [_, _, "outcome" | "conclusion"] => condition,
                    _ => false,
                }
        }
        "inputs" => parts.len() == 2 && scope.inputs.contains(&part(1)),
        "matrix" => {
            scope.level != Level::Job
                && scope.has_matrix
                && parts.len() == 2
                && is_identifier(part(1), true)
                && scope
                    .matrix_keys
                    .is_none_or(|keys| keys.iter().any(|key| key == part(1)))
        }
        "job" => scope.level == Level::Step && parts.len() == 2,
        "github" => {
            parts.len() >= 2
                && (condition
                    || matches!(parts.as_slice(), ["github", "token" | "repository" | "sha"]))
        }
        _ => false,
    }
}

const SECRET_PLACE: &str = "a secret may be read only as the whole value of a step env entry, in a workflow that runs on workflow_dispatch alone";

fn is_secret(reference: &str) -> bool {
    reference
        .split('.')
        .next()
        .is_some_and(|context| context.eq_ignore_ascii_case("secrets"))
}

/// A secret name: an upper case letter, then upper case letters, digits and underscores.
fn secret_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_uppercase())
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn secret_in(
    inner: &str,
    reference: &str,
    scope: &Scope<'_>,
    in_env: bool,
) -> Result<(), WorkflowsError> {
    if !in_env {
        return refuse(format!(
            "the expression {inner} reads {reference}, which is not allowed here; {SECRET_PLACE}"
        ));
    }
    if scope.events != ["workflow_dispatch"] {
        return refuse(format!(
            "the expression {inner} reads {reference}, but this workflow runs on {}; {SECRET_PLACE}",
            scope.events.join(", ")
        ));
    }
    let named = reference
        .strip_prefix("secrets.")
        .is_some_and(secret_name);
    if inner != reference || !named {
        return refuse(format!(
            "the expression {inner} must be exactly secrets.<NAME>, where NAME is an upper case letter followed by upper case letters, digits and underscores; {SECRET_PLACE}"
        ));
    }
    Ok(())
}

fn refused(inner: &str, reference: &str) -> WorkflowsError {
    if is_secret(reference) {
        return WorkflowsError(format!(
            "the expression {inner} reads {reference}, which is not allowed here; {SECRET_PLACE}"
        ));
    }
    WorkflowsError(format!(
        "the expression {inner} reads {reference}, which is not allowed here; use github.token, inputs.<declared input>, matrix.<key> in a matrix job, steps.<earlier id>.outputs.<name>, needs.<needed job>.outputs.<declared output>, and in an if also needs.<needed job>.result, steps.<id>.outcome and github.*"
    ))
}

pub(crate) fn check(value: &str, scope: &Scope<'_>) -> Result<(), WorkflowsError> {
    check_in(value, scope, false)
}

/// Like `check`, for the value of a step `env` entry, the one place a secret may be read.
pub(crate) fn check_env(value: &str, scope: &Scope<'_>) -> Result<(), WorkflowsError> {
    check_in(value, scope, true)
}

fn check_in(value: &str, scope: &Scope<'_>, in_env: bool) -> Result<(), WorkflowsError> {
    let Some(inner) = whole_expression(value) else {
        return refuse(format!(
            "the value {value:?} embeds an expression; a value is either plain text or exactly one expression"
        ));
    };
    let references = references(inner);
    if let Some(secret) = references.iter().find(|reference| is_secret(reference)) {
        return secret_in(inner, secret, scope, in_env);
    }
    match references
        .into_iter()
        .find(|reference| !allowed(reference, scope, false))
    {
        None => Ok(()),
        Some(reference) => Err(refused(inner, &reference)),
    }
}

pub(crate) fn check_condition(value: &str, scope: &Scope<'_>) -> Result<String, WorkflowsError> {
    if value.contains(char::is_control) {
        return refuse(format!(
            "the condition {value:?} must be one line; a line break or control character is refused"
        ));
    }
    let inner = bare(value);
    if inner.is_empty() || inner.contains("${{") || inner.contains("}}") {
        return refuse(format!(
            "the condition {value:?} must be one expression, with or without ${{{{ }}}}"
        ));
    }
    match references(inner)
        .into_iter()
        .find(|reference| !allowed(reference, scope, true))
    {
        None => Ok(inner.to_owned()),
        Some(reference) => Err(refused(inner, &reference)),
    }
}
