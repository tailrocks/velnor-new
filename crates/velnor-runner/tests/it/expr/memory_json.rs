// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{env_with, error_text, exceeded_memory, same_text, text};
use anyhow::Result;
use serde_json::json;
use velnor_runner::expr::{ExprEnv, RootContext};

const STRING_UNITS_THAT_FIT: usize = 5_242_865;
const NESTED_STRING_UNITS_THAT_FIT: usize = 5_242_815;
const COPIES_OF_EACH: usize = 1_000;

fn env_with_json(context: &serde_json::Value) -> Result<ExprEnv> {
    env_with(&[(RootContext::Env, &context.to_string())])
}

#[test]
fn expr_to_json_charges_each_escape_by_its_written_length() -> Result<()> {
    let expression = "toJSON(env.TEXT)";
    let characters = [
        ('a', 1),
        ('é', 1),
        ('\u{1f600}', 2),
        ('"', 2),
        ('\\', 2),
        ('\u{08}', 2),
        ('\u{0c}', 2),
        ('\n', 2),
        ('\r', 2),
        ('\t', 2),
        ('\u{01}', 6),
        ('\u{1f}', 6),
        ('\u{85}', 6),
        ('\u{2028}', 6),
        ('\u{2029}', 6),
    ];
    let mut every_kind = String::new();
    let mut units = 0;
    for (character, written) in characters {
        every_kind.push_str(&character.to_string().repeat(COPIES_OF_EACH));
        units += written * COPIES_OF_EACH;
    }
    let text_of = |total: usize| {
        let filler = "a".repeat(total - units);
        env_with_json(&json!({"TEXT": format!("{every_kind}{filler}")}))
    };

    text(expression, &text_of(STRING_UNITS_THAT_FIT)?)?;
    same_text(
        &error_text(expression, &text_of(STRING_UNITS_THAT_FIT + 1)?)?,
        &exceeded_memory(expression),
    )
}

#[test]
fn expr_to_json_charges_the_separators_written_before_a_nested_string() -> Result<()> {
    let expression = "toJSON(env.NESTED)";
    let nested = |units: usize| env_with_json(&json!({"NESTED": {"k": "a".repeat(units)}}));

    text(expression, &nested(NESTED_STRING_UNITS_THAT_FIT)?)?;
    same_text(
        &error_text(expression, &nested(NESTED_STRING_UNITS_THAT_FIT + 1)?)?,
        &exceeded_memory(expression),
    )
}
