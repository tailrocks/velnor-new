// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/tests.rs

use crate::expr_support::{empty_env, env_with, error_text, eval, text, truthy};
use anyhow::Result;
use velnor_runner::expr::{RootContext, Value};

#[test]
fn expr_operator_precedence_and_grouping() -> Result<()> {
    let env = empty_env();

    assert!(truthy("true || false && false", &env)?);
    assert!(!truthy("(true || false) && false", &env)?);
    assert!(truthy("1 < 2 == true", &env)?);
    assert!(truthy("!false && true", &env)?);
    assert!(!truthy("!(false && true) == false", &env)?);
    Ok(())
}

#[test]
fn expr_logical_operators_return_operand_values() -> Result<()> {
    let env = empty_env();

    assert_eq!(text("'a' && 'b'", &env)?, "b");
    assert_eq!(text("'' && 'b'", &env)?, "");
    assert_eq!(text("'' || 'b'", &env)?, "b");
    assert_eq!(text("'a' || 'b'", &env)?, "a");
    assert_eq!(text("'0' || 'b'", &env)?, "0");
    Ok(())
}

#[test]
fn expr_short_circuit_skips_right_operand() -> Result<()> {
    let env = env_with(&[(RootContext::Env, "{}")])?;

    assert!(matches!(
        eval("env.missing && env.missing.deeper", &env)?,
        Value::Null
    ));
    assert!(matches!(
        eval("true || format('{9}', 'x')", &env)?,
        Value::Boolean(true)
    ));
    assert!(matches!(
        eval("false && format('{9}', 'x')", &env)?,
        Value::Boolean(false)
    ));
    Ok(())
}

#[test]
fn expr_string_truthiness_only_empty_is_falsy() -> Result<()> {
    let env = env_with(&[(
        RootContext::Steps,
        r#"{"x":{"outputs":{"count":"0","flag":"false","blank":""}}}"#,
    )])?;

    assert!(truthy("steps.x.outputs.count", &env)?);
    assert!(truthy("steps.x.outputs.flag", &env)?);
    assert!(!truthy("steps.x.outputs.blank", &env)?);
    assert!(truthy("steps.x", &env)?);
    assert!(!truthy("0", &env)?);
    assert!(!truthy("NaN", &env)?);
    Ok(())
}

#[test]
fn expr_null_coerces_in_comparisons() -> Result<()> {
    let env = env_with(&[(RootContext::Env, r#"{"SET":"v"}"#)])?;

    for expression in [
        "env.UNSET == ''",
        "env.UNSET == null",
        "env.UNSET == 0",
        "env.UNSET == false",
        "env.SET == 'V'",
        "'AbC' == 'abc'",
        "true == 1",
        "1 == ' 1 '",
        "null == '0'",
    ] {
        assert!(truthy(expression, &env)?, "{expression}");
    }
    for expression in [
        "env.UNSET",
        "env.UNSET == 'v'",
        "1 == 'one'",
        "NaN == NaN",
        "github == 1",
    ] {
        let env = env_with(&[
            (RootContext::Env, r#"{"SET":"v"}"#),
            (RootContext::Github, "{}"),
        ])?;
        assert!(!truthy(expression, &env)?, "{expression}");
    }
    assert_eq!(text("env.UNSET", &env)?, "");
    Ok(())
}

#[test]
fn expr_relational_operators_follow_github_coercion() -> Result<()> {
    let env = env_with(&[(RootContext::Github, r#"{"run_number":"1"}"#)])?;

    for expression in [
        "github.run_number < 5",
        "github.run_number <= 1",
        "github.run_number >= 1",
        "2 > '1'",
        "'b' > 'A'",
        "true > false",
        "null >= 0",
    ] {
        assert!(truthy(expression, &env)?, "{expression}");
    }
    for expression in [
        "github.run_number > 5",
        "github.run_number >= 2",
        "1 > 'a'",
        "1 < 'a'",
        "github > 0",
        "github < 0",
    ] {
        assert!(!truthy(expression, &env)?, "{expression}");
    }
    Ok(())
}

#[test]
fn expr_numbers_format_and_parse_like_github() -> Result<()> {
    let env = empty_env();

    assert_eq!(text("0.30000000000000004", &env)?, "0.3");
    assert_eq!(text("0.1", &env)?, "0.1");
    assert_eq!(text("-5", &env)?, "-5");
    assert_eq!(text("0x1f", &env)?, "31");
    assert_eq!(text("0o17", &env)?, "15");
    assert_eq!(text("1e3", &env)?, "1000");
    assert_eq!(text("1e15", &env)?, "1E+15");
    assert_eq!(text("0.0001", &env)?, "0.0001");
    assert_eq!(text("0.00001", &env)?, "1E-05");
    assert_eq!(text("Infinity", &env)?, "Infinity");
    assert_eq!(text("-Infinity", &env)?, "-Infinity");
    assert_eq!(text("NaN", &env)?, "NaN");
    Ok(())
}

#[test]
fn expr_malformed_expressions_are_parse_errors() -> Result<()> {
    let env = empty_env();

    for expression in [
        "1 +",
        "(",
        "'unterminated",
        "==",
        "1 == == 2",
        "a..",
        "1 2",
        "github.sha)",
        "format('a'",
    ] {
        assert!(
            eval(expression, &env).is_err(),
            "expected a parse error for {expression:?}"
        );
    }
    assert!(error_text("contains('a')", &env)?.contains("Too few parameters"));
    assert!(error_text("toJSON('a', 'b')", &env)?.contains("Too many parameters"));
    assert!(matches!(eval("", &env)?, Value::Null));
    assert!(matches!(eval("   ", &env)?, Value::Null));
    Ok(())
}

#[test]
fn expr_hex_and_octal_literals_use_signed_32_bit_semantics() -> Result<()> {
    let env = empty_env();

    assert!(truthy("0xFFFFFFFF == -1", &env)?);
    assert_eq!(text("0x80000000", &env)?, "-2147483648");
    assert_eq!(text("0x7FFFFFFF", &env)?, "2147483647");
    assert_eq!(text("0o37777777777", &env)?, "-1");
    assert!(truthy("'0xFFFFFFFF' == -1", &env)?);
    for expression in ["0x100000000", "0o40000000000"] {
        assert!(
            error_text(expression, &env)?.starts_with("Unexpected symbol"),
            "{expression}"
        );
    }
    Ok(())
}

#[test]
fn expr_string_comparison_orders_utf16_code_units() -> Result<()> {
    let json = format!(r#"{{"A":"{}","B":"{}"}}"#, '\u{10000}', '\u{e000}');
    let env = env_with(&[(RootContext::Env, &json)])?;

    assert!(truthy("env.A < env.B", &env)?);
    assert!(truthy("env.B > env.A", &env)?);
    assert!(!truthy("env.A == env.B", &env)?);
    Ok(())
}
