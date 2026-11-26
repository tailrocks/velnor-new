// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/tests.rs

use crate::expr_support::{
    empty_env, env_with, error_text, exceeded_memory, text, truthy, within_the_linear_time_limit,
};
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
fn expr_format_reports_unescaped_specifiers_of_the_argument_kind() -> Result<()> {
    let env = empty_env();

    assert_eq!(text("format('{0:}', 'z')", &env)?, "z");
    assert_eq!(
        error_text("format('{0:x}', 1)", &env)?,
        "The format specifiers 'x' are not valid for objects of type 'Number'"
    );
    assert_eq!(
        error_text("format('{0:a}}b}', 'z')", &env)?,
        "The format specifiers 'a}b' are not valid for objects of type 'String'"
    );
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

#[test]
fn expr_from_json_accepts_only_a_string_literal() -> Result<()> {
    let env = env_with(&[(RootContext::Env, r#"{"PAYLOAD":"{}"}"#)])?;

    assert_eq!(text(r#"fromJSON('"\u007b"')"#, &env)?, "{");
    assert_eq!(
        text(r#"format('{0}{1}', 'a', fromJSON('"\u007d"'))"#, &env)?,
        "a}"
    );
    assert_eq!(
        text(r#"toJSON(fromJSON('[1,"x",{"k":null}]'))"#, &env)?,
        text(r#"toJSON(fromJSON('[1, "x", {"k": null}]'))"#, &env)?
    );
    assert!(truthy(r#"fromJSON('{"a":[true]}').a[0]"#, &env)?);
    assert!(truthy("fromJSON('true')", &env)?);
    assert_eq!(
        error_text("fromJSON(env.PAYLOAD)", &env)?,
        "unsupported expression function fromJSON"
    );
    assert_eq!(
        error_text("FROMJSON(format('{0}', 'x'))", &env)?,
        "unsupported expression function FROMJSON"
    );
    assert_eq!(
        error_text("fromJSON('a' && 'b')", &env)?,
        "unsupported expression function fromJSON"
    );
    assert!(error_text("fromJSON('a', 'b')", &env)?.contains("Too many parameters"));
    assert!(error_text("fromJSON('{')", &env)?.contains("Error parsing fromJson"));
    Ok(())
}

#[test]
fn expr_contains_scans_long_values_in_linear_time() -> Result<()> {
    within_the_linear_time_limit("contains over million-character values", || {
        let haystack = "a".repeat(1_000_000);
        let needle = format!("{}b", "a".repeat(500_000));
        let env = env_with(&[(
            RootContext::Env,
            &format!(
                r#"{{"HAYSTACK":"{haystack}","NEEDLE":"{needle}","UPPER":"{}"}}"#,
                "A".repeat(100_000)
            ),
        )])?;

        assert!(!truthy("contains(env.HAYSTACK, env.NEEDLE)", &env)?);
        assert!(truthy("contains(env.HAYSTACK, env.UPPER)", &env)?);
        Ok(())
    })
}

#[test]
fn expr_format_validates_the_format_string_before_resolving_arguments() -> Result<()> {
    let env = empty_env();

    assert_eq!(
        error_text("format('{0}{', hashFiles('**'))", &env)?,
        "Invalid format string: {0}{"
    );
    assert_eq!(
        error_text("format('{0:x}{', hashFiles('**'))", &env)?,
        "Invalid format string: {0:x}{"
    );
    assert!(
        error_text("format('{0}', hashFiles('**'))", &env)?.starts_with("hashFiles('**') failed")
    );
    Ok(())
}

#[test]
fn expr_to_json_stops_at_the_memory_budget() -> Result<()> {
    let items = |count: usize| format!(r#"{{"LIST":[{}0]}}"#, "1,".repeat(count));
    let within = env_with(&[(RootContext::Env, &items(100_000))])?;
    let beyond = env_with(&[(RootContext::Env, &items(400_000))])?;

    assert!(text("toJSON(env.LIST)", &within)?.starts_with("[\n  1,\n  1,"));
    assert_eq!(
        error_text("toJSON(env.LIST)", &beyond)?,
        exceeded_memory("toJSON(env.LIST)")
    );
    Ok(())
}

#[test]
fn expr_to_json_escapes_line_separators_like_json_net() -> Result<()> {
    let env = env_with(&[(
        RootContext::Env,
        r#"{"TEXT":"a\u0085b\u2028c\u2029d\u0001e\u001bf\u007f\"q\\","k\u2028":1}"#,
    )])?;
    let expected = format!(r#""a\u0085b\u2028c\u2029d\u0001e\u001bf{}\"q\\""#, '\u{7f}');

    assert_eq!(text("toJSON(env.TEXT)", &env)?, expected);
    assert!(text("toJSON(env)", &env)?.contains(r#""k\u2028": 1"#));
    Ok(())
}

#[test]
fn expr_format_scans_long_format_strings_in_linear_time() -> Result<()> {
    within_the_linear_time_limit("format over 600,000-character format strings", || {
        let env = env_with(&[(
            RootContext::Env,
            &format!(
                r#"{{"OPEN":"{}","CLOSE":"{}"}}"#,
                "{{".repeat(300_000),
                "}}".repeat(300_000)
            ),
        )])?;

        assert_eq!(text("format(env.OPEN)", &env)?.len(), 300_000);
        assert_eq!(text("format(env.CLOSE)", &env)?.len(), 300_000);
        Ok(())
    })
}
