// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Value};
use velnor_model::{CACHE_COMPAT_LENGTH, RUNNER_ARCH_MAX_LENGTH, RUNNER_OS_MAX_LENGTH};

const CACHE_ACTIONS: [&str; 3] = [
    "actions/cache",
    "actions/cache/restore",
    "actions/cache/save",
];

pub(crate) fn longest_cache_key(step: &RenderedStep) -> Option<usize> {
    let uses = step.uses.as_deref()?.split('@').next()?;
    if !CACHE_ACTIONS.contains(&uses) {
        return None;
    }
    step.with
        .iter()
        .filter(|(name, _)| name == "key" || name == "restore-keys")
        .filter_map(|(_, value)| match value {
            Value::Expr(text) | Value::Text(text) => Some(text),
            _ => None,
        })
        .flat_map(|text| text.lines())
        .map(expanded_length)
        .max()
}

fn expanded_length(line: &str) -> usize {
    let mut total = 0;
    let mut rest = line;
    while let Some((literal, after)) = rest.split_once("${{") {
        total += literal.encode_utf16().count();
        let (expression, tail) = after.split_once("}}").unwrap_or((after, ""));
        total += expansion(expression.trim());
        rest = tail;
    }
    total + rest.encode_utf16().count()
}

fn expansion(expression: &str) -> usize {
    match expression {
        "runner.os" => RUNNER_OS_MAX_LENGTH,
        "runner.arch" => RUNNER_ARCH_MAX_LENGTH,
        "github.sha" => 40,
        "github.run_id" => 20,
        "github.run_attempt" => 10,
        "github.job" => 100,
        _ if expression.starts_with("hashFiles(") || expression.contains(".outputs.lock_") => 64,
        _ if expression.ends_with(".outputs.compat") => CACHE_COMPAT_LENGTH,
        _ if expression.ends_with(".outputs.base8") => 8,
        _ => expression.encode_utf16().count(),
    }
}
