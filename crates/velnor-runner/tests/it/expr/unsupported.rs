// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{empty_env, error_text};
use anyhow::Result;
use velnor_runner::expr::evaluate_expression;

#[test]
fn expr_unsupported_function_is_error() -> Result<()> {
    let env = empty_env();

    assert_eq!(
        error_text("toYAML(env.PAYLOAD)", &env)?,
        "unsupported expression function toYAML"
    );
    assert_eq!(
        error_text("join('a', ',')", &env)?,
        "unsupported expression function join"
    );
    assert_eq!(
        error_text("case(true, 'a', 'b')", &env)?,
        "unsupported expression function case"
    );
    assert_eq!(
        error_text("1 == 1 && noSuchFunction()", &env)?,
        "unsupported expression function noSuchFunction"
    );
    Ok(())
}

#[test]
fn expr_unsupported_context_is_error() -> Result<()> {
    let env = empty_env();

    for context in ["job", "jobs", "matrix", "strategy"] {
        assert_eq!(
            error_text(&format!("{context}.x"), &env)?,
            format!("unsupported expression context {context}")
        );
    }
    assert_eq!(
        error_text("format('{0}', Matrix.x)", &env)?,
        "unsupported expression context Matrix"
    );
    for context in [
        "github", "runner", "needs", "steps", "inputs", "secrets", "env", "vars", "GITHUB",
    ] {
        evaluate_expression(&format!("{context}.x"), &env)?;
    }
    Ok(())
}
