// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared::DeclaredRule;

pub(crate) struct Scope<'a> {
    pub(crate) needs: &'a [String],
    pub(crate) steps: &'a [String],
    pub(crate) condition: bool,
}

pub(crate) fn bare(text: &str) -> &str {
    text.trim()
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .map_or_else(|| text.trim(), str::trim)
}

fn references(expression: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    for c in expression.chars().chain(std::iter::once(' ')) {
        if c == '\'' {
            quoted = !quoted;
        }
        let word = !quoted && c != '\'' && (c.is_ascii_alphanumeric() || "_.*-".contains(c));
        if word {
            token.push(c);
        } else {
            if token.contains('.') && token.starts_with(|first: char| first.is_ascii_alphabetic()) {
                found.push(token.clone());
            }
            token.clear();
        }
    }
    found
}

fn allowed(reference: &str, scope: &Scope<'_>) -> Result<(), DeclaredRule> {
    let parts: Vec<&str> = reference.split('.').collect();
    let part = |index: usize| parts.get(index).copied().unwrap_or("");
    let leaf = |extra: &[&str]| {
        (parts.len() == 4 && part(2) == "outputs")
            || (scope.condition && parts.len() == 3 && extra.contains(&part(2)))
    };
    let refused = || DeclaredRule::ContextNotAllowed(reference.to_owned());
    match part(0) {
        "needs" if scope.condition && part(1) == "*" => Ok(()),
        "needs" if !scope.needs.iter().any(|need| need == part(1)) => {
            Err(DeclaredRule::NeedNotNamed(part(1).to_owned()))
        }
        "needs" => leaf(&["result"]).then_some(()).ok_or_else(refused),
        "steps" if !scope.steps.iter().any(|step| step == part(1)) => {
            Err(DeclaredRule::UndeclaredStep(part(1).to_owned()))
        }
        "steps" => leaf(&["outcome", "conclusion"])
            .then_some(())
            .ok_or_else(refused),
        "job" => Ok(()),
        "github" if scope.condition || reference == "github.token" => Ok(()),
        _ => Err(refused()),
    }
}

pub(crate) fn check_expression(expression: &str, scope: &Scope<'_>) -> Vec<DeclaredRule> {
    references(expression)
        .iter()
        .filter_map(|reference| allowed(reference, scope).err())
        .collect()
}

pub(crate) fn check_text(text: &str, scope: &Scope<'_>) -> Vec<DeclaredRule> {
    let mut rules = Vec::new();
    let mut rest = text;
    while let Some((_, after)) = rest.split_once("${{") {
        let Some((inner, next)) = after.split_once("}}") else {
            rules.push(DeclaredRule::UnclosedExpression(text.to_owned()));
            break;
        };
        rules.extend(check_expression(inner, scope));
        rest = next;
    }
    rules
}
