// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};

pub(crate) struct Scope<'a> {
    pub(crate) inputs: &'a [&'a str],
    pub(crate) has_matrix: bool,
    pub(crate) job_level: bool,
    pub(crate) needs: &'a [String],
    pub(crate) needed_outputs: &'a [(&'a str, &'a str)],
    pub(crate) steps: Vec<String>,
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
        "steps" if scope.job_level => false,
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
            !scope.job_level && scope.has_matrix && parts.len() == 2 && is_identifier(part(1), true)
        }
        "job" => !scope.job_level && parts.len() == 2,
        "github" => {
            parts.len() >= 2
                && (condition
                    || matches!(parts.as_slice(), ["github", "token" | "repository" | "sha"]))
        }
        _ => false,
    }
}

fn refused(inner: &str, reference: &str) -> WorkflowsError {
    WorkflowsError(format!(
        "the expression {inner} reads {reference}, which is not allowed here; use github.token, inputs.<declared input>, matrix.<key> in a matrix job, steps.<earlier id>.outputs.<name>, needs.<needed job>.outputs.<declared output>, and in an if also needs.<needed job>.result, steps.<id>.outcome and github.*"
    ))
}

pub(crate) fn check(value: &str, scope: &Scope<'_>) -> Result<(), WorkflowsError> {
    let Some(inner) = whole_expression(value) else {
        return refuse(format!(
            "the value {value:?} embeds an expression; a value is either plain text or exactly one expression"
        ));
    };
    match references(inner)
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
