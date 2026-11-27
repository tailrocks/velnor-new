// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use crate::expr_support::{empty_env, env_with, template_error_text};
use anyhow::Result;
use velnor_runner::expr::{RootContext, evaluate_template};

#[test]
fn expr_template_substitutes_every_span() -> Result<()> {
    let env = env_with(&[
        (
            RootContext::Github,
            r#"{"sha":"deadbeef","ref":"refs/heads/main"}"#,
        ),
        (RootContext::Inputs, r#"{"name":"velnor"}"#),
    ])?;

    assert_eq!(evaluate_template("plain text", &env)?, "plain text");
    assert_eq!(evaluate_template("${{ github.sha }}", &env)?, "deadbeef");
    assert_eq!(
        evaluate_template("cache-${{github.sha}}-${{ inputs.name }}-v1", &env)?,
        "cache-deadbeef-velnor-v1"
    );
    assert_eq!(evaluate_template("[${{ github.missing }}]", &env)?, "[]");
    assert_eq!(evaluate_template("[${{ }}]", &env)?, "[]");
    assert_eq!(
        evaluate_template(
            "${{ startsWith(github.ref, 'refs/heads/') && 'branch' || 'other' }}",
            &env
        )?,
        "branch"
    );
    Ok(())
}

#[test]
fn expr_template_keeps_closing_braces_inside_string_literals() -> Result<()> {
    let env = empty_env();

    assert_eq!(
        evaluate_template("${{ format('{{{0}}}', 'x') }}!", &env)?,
        "{x}!"
    );
    assert_eq!(evaluate_template("${{ 'a}}b' }}", &env)?, "a}}b");
    assert_eq!(evaluate_template("${{ 'it''s' }}", &env)?, "it's");
    Ok(())
}

#[test]
fn expr_template_reports_unclosed_and_invalid_expressions() {
    let env = empty_env();

    let unclosed = evaluate_template("a ${{ github.sha", &env).map_err(|error| error.to_string());
    assert_eq!(
        unclosed,
        Err("The expression is not closed. An unescaped ${{ sequence was found, but the closing }} sequence was not found.".to_string())
    );
    let unsupported = evaluate_template("x ${{ vars.a }}", &env).map_err(|error| error.to_string());
    assert_eq!(
        unsupported,
        Err("unsupported expression context vars".to_string())
    );
}

const EXPRESSION_LIMIT_UNITS: usize = 21_000;
const FORMAT_FRAME_UNITS: usize = "format('{0}', 'x')".len();

#[test]
fn expr_template_limits_the_rebuilt_format_to_21000_utf16_units() -> Result<()> {
    let env = empty_env();
    let cases = [("a", 1), ("\u{1F600}", 2), ("{", 2), ("}", 2), ("'", 2)];
    for (character, units) in cases {
        let fits = (EXPRESSION_LIMIT_UNITS - FORMAT_FRAME_UNITS) / units;
        let rendered =
            evaluate_template(&format!("{}${{{{ 'x' }}}}", character.repeat(fits)), &env)?;
        assert_eq!(rendered.chars().count(), fits + 1, "{character:?}");
        assert_eq!(
            template_error_text(
                &format!("{}${{{{ 'x' }}}}", character.repeat(fits + 1)),
                &env
            )?,
            "Exceeded max expression length 21000",
            "{character:?}"
        );
    }
    Ok(())
}

#[test]
fn expr_template_counts_the_trimmed_span_source_in_the_rebuilt_format() -> Result<()> {
    let env = empty_env();
    let span = |body: &str, padding: usize| {
        format!(
            "a${{{{{}{body}{}}}}}b",
            " ".repeat(padding),
            " ".repeat(padding)
        )
    };
    let longest_fitting = EXPRESSION_LIMIT_UNITS - "format('a{0}b', '')".len();

    assert_eq!(evaluate_template(&span("'x'", 25_000), &env)?, "axb");
    assert_eq!(
        evaluate_template(
            &span(&format!("'{}'", "x".repeat(longest_fitting)), 0),
            &env
        )?
        .len(),
        longest_fitting + 2
    );
    assert_eq!(
        template_error_text(
            &span(&format!("'{}'", "x".repeat(longest_fitting + 1)), 0),
            &env
        )?,
        "Exceeded max expression length 21000"
    );
    Ok(())
}

#[test]
fn expr_template_without_a_span_is_returned_uncharged() -> Result<()> {
    let literal = "a".repeat(6_000_000);

    assert_eq!(evaluate_template(&literal, &empty_env())?.len(), 6_000_000);
    Ok(())
}

#[test]
fn expr_template_parses_every_span_before_it_evaluates_any() -> Result<()> {
    let env = env_with(&[(
        RootContext::Env,
        &format!(r#"{{"PAYLOAD":"{}"}}"#, "x".repeat(100_000)),
    )])?;
    let source = format!("{}${{{{ vars.a }}}}", "${{ env.PAYLOAD }}".repeat(53));

    assert_eq!(
        template_error_text(&source, &env)?,
        "unsupported expression context vars"
    );
    Ok(())
}
