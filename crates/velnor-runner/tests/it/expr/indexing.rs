// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/tests.rs

use crate::expr_support::{
    empty_env, env_with, error_text, eval, exceeded_memory, same_text, text, truthy,
};
use anyhow::{Context, Result};
use velnor_runner::context_value::ContextValue;
use velnor_runner::expr::{RootContext, Value};

const GITHUB: &str = r#"{"event":{"commits":[{"id":"a"},{"id":"b"}]},"list":[10,20]}"#;

#[test]
fn expr_index_and_dereference_read_contexts() -> Result<()> {
    let env = env_with(&[
        (RootContext::Github, GITHUB),
        (RootContext::Env, r#"{"A":"1"}"#),
    ])?;

    assert_eq!(text("github.event.commits[0].id", &env)?, "a");
    assert_eq!(text("github['event']['commits'][1]['id']", &env)?, "b");
    assert_eq!(text("github.EVENT.COMMITS[0].ID", &env)?, "a");
    assert_eq!(text("github.list[1]", &env)?, "20");
    assert_eq!(text("github.list['1']", &env)?, "20");
    assert!(matches!(eval("github.missing", &env)?, Value::Null));
    assert!(matches!(eval("github.missing.deeper", &env)?, Value::Null));
    assert!(matches!(eval("env.A.nope", &env)?, Value::Null));
    assert!(matches!(
        eval("github.event.commits[9]", &env)?,
        Value::Null
    ));
    assert!(matches!(eval("github.list[-1]", &env)?, Value::Null));
    assert!(matches!(eval("github.list[github]", &env)?, Value::Null));
    Ok(())
}

#[test]
fn expr_wildcard_filters_arrays_and_objects() -> Result<()> {
    let env = env_with(&[(RootContext::Github, GITHUB)])?;

    assert!(truthy("contains(github.event.commits.*.id, 'a')", &env)?);
    assert!(truthy("contains(github.event.commits[*].id, 'b')", &env)?);
    assert!(!truthy("contains(github.event.commits.*.id, 'c')", &env)?);
    assert!(!truthy("contains(github.missing.*, 'a')", &env)?);
    assert_eq!(
        text("toJSON(github.event.commits.*.id)", &env)?,
        "[\n  \"a\",\n  \"b\"\n]"
    );
    assert!(text("toJSON(github.event.*)", &env)?.contains("\"id\": \"a\""));
    assert_eq!(text("toJSON(github.missing.*)", &env)?, "[]");
    Ok(())
}

#[test]
fn expr_env_context_keeps_case_sensitive_keys() -> Result<()> {
    let mut env = empty_env();
    let value = ContextValue::from_json_str_case_sensitive(r#"{"Path":"first","path":"second"}"#)
        .context("parse env json")?;
    env.set_context(RootContext::Env, &value);

    assert_eq!(text("env.Path", &env)?, "first");
    assert_eq!(text("env.path", &env)?, "second");
    assert_eq!(text("env.PATH", &env)?, "");
    Ok(())
}

#[test]
fn expr_context_values_convert_numbers_and_non_finite() -> Result<()> {
    let env = env_with(&[(
        RootContext::Inputs,
        r#"{"count":3,"big":18446744073709551615,"ratio":1.5,"flag":true,"none":null}"#,
    )])?;

    assert_eq!(text("inputs.count", &env)?, "3");
    assert_eq!(text("inputs.big", &env)?, "1.84467440737096E+19");
    assert_eq!(text("inputs.ratio", &env)?, "1.5");
    assert!(truthy("inputs.flag", &env)?);
    assert!(!truthy("inputs.none", &env)?);
    assert!(truthy("inputs.count == '3'", &env)?);
    Ok(())
}

#[test]
fn expr_wildcard_projections_stop_at_the_memory_budget() -> Result<()> {
    let list = |count: usize| format!(r#"{{"LIST":[{}0]}}"#, "1,".repeat(count));
    let within = env_with(&[(RootContext::Env, &list(1_000_000))])?;
    let beyond = env_with(&[(RootContext::Env, &list(1_400_000))])?;

    assert!(truthy("env.LIST.* != null", &within)?);
    same_text(
        &error_text("env.LIST.*", &beyond)?,
        &exceeded_memory("env.LIST[*]"),
    )?;
    Ok(())
}
