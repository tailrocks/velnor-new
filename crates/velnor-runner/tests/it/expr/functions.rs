// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/tests.rs

use crate::expr_support::{empty_env, env_with, error_text, text, truthy};
use anyhow::Result;
use velnor_runner::expr::RootContext;

#[test]
fn expr_format_substitutes_arguments() -> Result<()> {
    let env = env_with(&[(RootContext::Github, r#"{"sha":"deadbeef"}"#)])?;

    assert_eq!(text("format('{0}-{1}', 'a', 'b')", &env)?, "a-b");
    assert_eq!(
        text("format('{{literal}} {0}', github.sha)", &env)?,
        "{literal} deadbeef"
    );
    assert_eq!(text("format('{0}{0}', 'x')", &env)?, "xx");
    assert_eq!(text("format('[{0}]', github.missing)", &env)?, "[]");
    assert_eq!(text("format('{0}', 1.5)", &env)?, "1.5");
    assert_eq!(text("format('{0}', true)", &env)?, "true");
    assert!(error_text("format('{1}', 'a')", &env)?.contains("references more arguments"));
    assert!(error_text("format('{0')", &env)?.contains("Invalid format string"));
    Ok(())
}

#[test]
fn expr_contains_string_and_array() -> Result<()> {
    let env = env_with(&[(RootContext::Github, r#"{"list":["a",2],"empty":[]}"#)])?;

    assert!(truthy("contains('Hello world', 'WORLD')", &env)?);
    assert!(!truthy("contains('Hello', 'x')", &env)?);
    assert!(truthy("contains('abc', '')", &env)?);
    assert!(truthy("contains(github.list, 'A')", &env)?);
    assert!(truthy("contains(github.list, '2')", &env)?);
    assert!(!truthy("contains(github.list, 'z')", &env)?);
    assert!(!truthy("contains(github.empty, 'a')", &env)?);
    assert!(!truthy("contains('abc', github.list)", &env)?);
    Ok(())
}

#[test]
fn expr_starts_with_ignores_case() -> Result<()> {
    let env = env_with(&[(RootContext::Github, r#"{"ref":"refs/tags/V1"}"#)])?;

    assert!(truthy("startsWith(github.ref, 'refs/tags/')", &env)?);
    assert!(truthy("startsWith(github.ref, 'REFS/TAGS/v')", &env)?);
    assert!(!truthy("startsWith(github.ref, 'refs/heads/')", &env)?);
    assert!(truthy("startsWith('abc', '')", &env)?);
    assert!(!truthy("startsWith('ab', 'abc')", &env)?);
    assert!(!truthy("startsWith(github, 'x')", &env)?);
    Ok(())
}

#[test]
fn expr_to_json_pretty_prints_object() -> Result<()> {
    let env = env_with(&[(RootContext::Github, r#"{"a":1,"b":["x"],"c":{},"d":null}"#)])?;

    assert_eq!(
        text("toJSON(github)", &env)?,
        "{\n  \"a\": 1,\n  \"b\": [\n    \"x\"\n  ],\n  \"c\": {},\n  \"d\": null\n}"
    );
    assert_eq!(text("toJSON('x')", &env)?, "\"x\"");
    assert_eq!(text("toJSON(null)", &empty_env())?, "null");
    assert_eq!(text("toJSON(github.b)", &env)?, "[\n  \"x\"\n]");
    Ok(())
}
