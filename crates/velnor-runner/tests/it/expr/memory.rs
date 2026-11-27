// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{
    env_with, error_text, eval, exceeded_memory, same_text, template_error_text, text,
};
use anyhow::Result;
use std::fmt::Write as _;
use velnor_runner::expr::{ExprEnv, RootContext, evaluate_template};

const PAYLOAD_CHARS: usize = 100_000;
const COPIES_THAT_FIT: usize = 52;
const COPIES_THAT_EXCEED: usize = 53;
const TEXT_THAT_FITS: usize = 5_200_000;
const TEXT_THAT_EXCEEDS: usize = 5_300_000;
const TIGHT_PAYLOAD_CHARS: usize = 104_800;
const TIGHT_COPIES: usize = 50;
const TEMPLATE_TEXT_THAT_FITS: usize = 2_200;
const TEMPLATE_TEXT_THAT_EXCEEDS: usize = 2_300;

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

fn rebuilt_format(head: &str, count: usize, tail: &str) -> String {
    format!(
        "format('{head}{}{tail}'{})",
        placeholders(count),
        ", env.PAYLOAD".repeat(count)
    )
}

fn template_length(source: &str, env: &ExprEnv) -> Result<usize> {
    Ok(evaluate_template(source, env)?.len())
}

#[test]
fn expr_format_arguments_stop_at_the_budget_after_fifty_two_copies() -> Result<()> {
    let env = env_with_key("PAYLOAD", &"x".repeat(PAYLOAD_CHARS))?;
    let copies = |count: usize| format!("format('{}', env.PAYLOAD)", "{0}".repeat(count));

    assert_eq!(
        text(&copies(COPIES_THAT_FIT), &env)?.len(),
        COPIES_THAT_FIT * PAYLOAD_CHARS
    );
    same_text(
        &error_text(&copies(COPIES_THAT_EXCEED), &env)?,
        &exceeded_memory(&copies(COPIES_THAT_EXCEED)),
    )
}

#[test]
fn expr_template_values_stop_at_the_budget_after_fifty_two_copies() -> Result<()> {
    let env = env_with_key("PAYLOAD", &"x".repeat(PAYLOAD_CHARS))?;
    let template = |count: usize| "${{ env['PAYLOAD'] }}".repeat(count);

    assert_eq!(
        template_length(&template(COPIES_THAT_FIT), &env)?,
        COPIES_THAT_FIT * PAYLOAD_CHARS
    );
    same_text(
        &template_error_text(&template(COPIES_THAT_EXCEED), &env)?,
        &exceeded_memory(&rebuilt_format("", COPIES_THAT_EXCEED, "")),
    )
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
        same_text(
            &error_text(expression, &exceeds)?,
            &exceeded_memory(expression),
        )?;
    }
    Ok(())
}

#[test]
fn expr_format_charges_text_before_it_reads_the_rest_of_the_format() -> Result<()> {
    let env = env_with_key("TEXT", &format!("{}{{{{{{", "a".repeat(TEXT_THAT_EXCEEDS)))?;

    same_text(
        &error_text("format(env.TEXT)", &env)?,
        &exceeded_memory("format(env.TEXT)"),
    )
}

#[test]
fn expr_template_charges_literal_text_before_and_after_a_span() -> Result<()> {
    let env = env_with_key("PAYLOAD", &"x".repeat(TIGHT_PAYLOAD_CHARS))?;
    let spans = "${{ env.PAYLOAD }}".repeat(TIGHT_COPIES);
    let head = |count: usize| format!("{}{spans}", "a".repeat(count));
    let tail = |count: usize| format!("{spans}{}", "a".repeat(count));
    let values = TIGHT_COPIES * TIGHT_PAYLOAD_CHARS;

    assert_eq!(
        template_length(&head(TEMPLATE_TEXT_THAT_FITS), &env)?,
        values + TEMPLATE_TEXT_THAT_FITS
    );
    same_text(
        &template_error_text(&head(TEMPLATE_TEXT_THAT_EXCEEDS), &env)?,
        &exceeded_memory(&rebuilt_format(
            &"a".repeat(TEMPLATE_TEXT_THAT_EXCEEDS),
            TIGHT_COPIES,
            "",
        )),
    )?;
    assert_eq!(
        template_length(&tail(TEMPLATE_TEXT_THAT_FITS), &env)?,
        values + TEMPLATE_TEXT_THAT_FITS
    );
    same_text(
        &template_error_text(&tail(TEMPLATE_TEXT_THAT_EXCEEDS), &env)?,
        &exceeded_memory(&rebuilt_format(
            "",
            TIGHT_COPIES,
            &"a".repeat(TEMPLATE_TEXT_THAT_EXCEEDS),
        )),
    )
}

#[test]
fn expr_memory_error_renders_the_expression_like_the_runner() -> Result<()> {
    let env = env_with_key("TEXT", &"a".repeat(TEXT_THAT_EXCEEDS))?;
    let arguments = "'it''s', (1 == 2) || !true, env['a b'], 1.5, null, GitHub.sha, env.x.*, \
                     env.a-b, Contains('a', 'b') && 0x10 != 16";
    let rendered = "'it''s', ((1 == 2) || !true), env['a b'], 1.5, null, GitHub.sha, env.x[*], \
                    env.a-b, (Contains('a', 'b') && (16 != 16))";

    same_text(
        &error_text(&format!("format(env.TEXT, {arguments})"), &env)?,
        &exceeded_memory(&format!("format(env.TEXT, {rendered})")),
    )
}

type EnvBuilder = fn(usize) -> Result<ExprEnv>;

fn flat_text_env(units: usize) -> Result<ExprEnv> {
    env_with_key("TEXT", &"a".repeat(units))
}

fn nested_text_env(units: usize) -> Result<ExprEnv> {
    env_with(&[(
        RootContext::Env,
        &format!(r#"{{"A":{{"K":"{}"}}}}"#, "a".repeat(units)),
    )])
}

#[test]
fn expr_wildcard_projection_charges_the_strings_it_copies() -> Result<()> {
    let cases: [(&str, &str, EnvBuilder); 2] = [
        ("env.*", "env[*]", flat_text_env),
        ("env.*.K", "env[*].K", nested_text_env),
    ];
    for (expression, rendered, build) in cases {
        eval(expression, &build(TEXT_THAT_FITS)?)?;
        same_text(
            &error_text(expression, &build(TEXT_THAT_EXCEEDS)?)?,
            &exceeded_memory(rendered),
        )?;
    }
    Ok(())
}
