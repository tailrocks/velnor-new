// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{env_with, error_text, exceeded_memory, text};
use anyhow::Result;
use std::fmt::Write as _;
use velnor_runner::expr::{ExprEnv, RootContext, evaluate_template};

const PAYLOAD_CHARS: usize = 100_000;
const COPIES_THAT_FIT: usize = 52;
const COPIES_THAT_EXCEED: usize = 53;
const TEXT_THAT_FITS: usize = 5_200_000;
const TEXT_THAT_EXCEEDS: usize = 5_300_000;
const EXPRESSION_LIMIT: usize = 21_000;

fn env_with_key(key: &str, value: &str) -> Result<ExprEnv> {
    env_with(&[(RootContext::Env, &format!(r#"{{"{key}":"{value}"}}"#))])
}

fn placeholders(count: usize) -> String {
    let mut out = String::new();
    for index in 0..count {
        let _ = write!(out, "{{{index}}}");
    }
    out
}

fn template_outcome(source: &str, env: &ExprEnv) -> Result<usize, String> {
    evaluate_template(source, env)
        .map(|rendered| rendered.len())
        .map_err(|error| error.to_string())
}

#[test]
fn expr_format_arguments_stop_at_the_budget_after_fifty_two_copies() -> Result<()> {
    let env = env_with_key("PAYLOAD", &"x".repeat(PAYLOAD_CHARS))?;
    let copies = |count: usize| format!("format('{}', env.PAYLOAD)", "{0}".repeat(count));

    assert_eq!(
        text(&copies(COPIES_THAT_FIT), &env)?.len(),
        COPIES_THAT_FIT * PAYLOAD_CHARS
    );
    assert_eq!(
        error_text(&copies(COPIES_THAT_EXCEED), &env)?,
        exceeded_memory(&copies(COPIES_THAT_EXCEED))
    );
    Ok(())
}

#[test]
fn expr_template_values_stop_at_the_budget_after_fifty_two_copies() -> Result<()> {
    let env = env_with_key("PAYLOAD", &"x".repeat(PAYLOAD_CHARS))?;
    let template = |count: usize| "${{ env.PAYLOAD }}".repeat(count);
    let rewritten = format!(
        "format('{}'{})",
        placeholders(COPIES_THAT_EXCEED),
        ", env.PAYLOAD".repeat(COPIES_THAT_EXCEED)
    );

    assert_eq!(
        template_outcome(&template(COPIES_THAT_FIT), &env),
        Ok(COPIES_THAT_FIT * PAYLOAD_CHARS)
    );
    assert_eq!(
        template_outcome(&template(COPIES_THAT_EXCEED), &env),
        Err(exceeded_memory(&rewritten))
    );
    Ok(())
}

#[test]
fn expr_format_charges_literal_text_of_every_kind() -> Result<()> {
    let cases = [("", 0), ("{0}", 1), ("{{", 1), ("}}", 1)];
    for (suffix, extra) in cases {
        let fits = env_with_key("TEXT", &format!("{}{suffix}", "a".repeat(TEXT_THAT_FITS)))?;
        let exceeds = env_with_key(
            "TEXT",
            &format!("{}{suffix}", "a".repeat(TEXT_THAT_EXCEEDS)),
        )?;
        let expression = "format(env.TEXT, 'x')";

        assert_eq!(
            text(expression, &fits)?.len(),
            TEXT_THAT_FITS + extra,
            "suffix {suffix:?}"
        );
        assert_eq!(
            error_text(expression, &exceeds)?,
            exceeded_memory(expression),
            "suffix {suffix:?}"
        );
    }
    Ok(())
}

#[test]
fn expr_format_charges_text_before_it_reads_the_rest_of_the_format() -> Result<()> {
    let env = env_with_key("TEXT", &format!("{}{{{{{{", "a".repeat(TEXT_THAT_EXCEEDS)))?;

    assert_eq!(
        error_text("format(env.TEXT)", &env)?,
        exceeded_memory("format(env.TEXT)")
    );
    Ok(())
}

#[test]
fn expr_template_charges_literal_text_before_and_after_a_span() {
    let span = "${{ 'x' }}";
    let head = |count: usize| format!("{}{span}", "a".repeat(count));
    let tail = |count: usize| format!("{span}{}", "a".repeat(count));
    let env = ExprEnv::new(std::path::PathBuf::from("/nonexistent-workspace"));
    let truncated = |prefix: &str| {
        let kept: String = format!("{prefix}{}", "a".repeat(EXPRESSION_LIMIT))
            .chars()
            .take(EXPRESSION_LIMIT)
            .collect();
        exceeded_memory(&format!("{kept}..."))
    };

    assert_eq!(
        template_outcome(&head(TEXT_THAT_FITS), &env),
        Ok(TEXT_THAT_FITS + 1)
    );
    assert_eq!(
        template_outcome(&head(TEXT_THAT_EXCEEDS), &env),
        Err(truncated("format('"))
    );
    assert_eq!(
        template_outcome(&tail(TEXT_THAT_FITS), &env),
        Ok(TEXT_THAT_FITS + 1)
    );
    assert_eq!(
        template_outcome(&tail(TEXT_THAT_EXCEEDS), &env),
        Err(truncated("format('{0}"))
    );
}

#[test]
fn expr_memory_error_renders_the_expression_like_the_runner() -> Result<()> {
    let env = env_with_key("TEXT", &"a".repeat(TEXT_THAT_EXCEEDS))?;
    let arguments = "'it''s', (1 == 2) || !true, env['a b'], 1.5, null, GitHub.sha, env.x.*, \
                     env.a-b, Contains('a', 'b') && 0x10 != 16";
    let rendered = "'it''s', ((1 == 2) || !true), env['a b'], 1.5, null, GitHub.sha, env.x[*], \
                    env.a-b, (Contains('a', 'b') && (16 != 16))";

    assert_eq!(
        error_text(&format!("format(env.TEXT, {arguments})"), &env)?,
        exceeded_memory(&format!("format(env.TEXT, {rendered})"))
    );
    Ok(())
}
