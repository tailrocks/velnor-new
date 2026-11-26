// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_expression;

const OPEN: &str = "${{";
const CLOSE: &str = "}}";

fn find_close(source: &str) -> Option<usize> {
    let mut in_string = false;
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => in_string = !in_string,
            b'}' if !in_string && bytes.get(index + 1) == Some(&b'}') => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

pub fn evaluate_template(source: &str, env: &ExprEnv) -> Result<String, ExprError> {
    let mut rendered = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find(OPEN) {
        rendered.push_str(&rest[..start]);
        let expression = &rest[start + OPEN.len()..];
        let Some(end) = find_close(expression) else {
            return Err(ExprError::evaluation(format!(
                "The expression is not closed. An unescaped {OPEN} sequence was found, but the closing {CLOSE} sequence was not found."
            )));
        };
        rendered.push_str(&evaluate_expression(expression[..end].trim(), env)?.convert_to_string());
        rest = &expression[end + CLOSE.len()..];
    }
    rendered.push_str(rest);
    Ok(rendered)
}
