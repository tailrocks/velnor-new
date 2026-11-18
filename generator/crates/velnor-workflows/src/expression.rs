// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};

pub(crate) struct Scope<'a> {
    pub(crate) inputs: &'a [&'a str],
    pub(crate) has_matrix: bool,
    pub(crate) needed_outputs: &'a [(&'a str, &'a str)],
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

pub(crate) fn check(value: &str, scope: &Scope<'_>) -> Result<(), WorkflowsError> {
    let Some(inner) = whole_expression(value) else {
        return refuse(format!(
            "the value {value:?} embeds an expression; a value is either plain text or exactly one expression"
        ));
    };
    let allowed = match inner.split('.').collect::<Vec<_>>().as_slice() {
        ["github", "token" | "repository" | "sha"] => true,
        ["inputs", name] => scope.inputs.contains(name),
        ["matrix", key] => scope.has_matrix && is_identifier(key, true),
        ["needs", job, "outputs", output] => scope.needed_outputs.contains(&(*job, *output)),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        refuse(format!(
            "the expression {inner} is not allowed here; use github.token, inputs.<declared input>, matrix.<key> in a matrix job, or needs.<needed job>.outputs.<declared output>"
        ))
    }
}
