// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use crate::expr_support::{empty_env, env_with};
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
