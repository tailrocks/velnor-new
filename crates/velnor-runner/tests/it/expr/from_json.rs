// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{empty_env, env_with, error_text, text};
use anyhow::Result;
use serde_json::json;
use velnor_runner::expr::{ExprEnv, RootContext};

const PREFIX: &str = "Error parsing fromJson:";

fn env_with_json(json: &str) -> Result<ExprEnv> {
    env_with(&[(RootContext::Env, &json!({ "J": json }).to_string())])
}

fn read(json: &str, suffix: &str) -> Result<String> {
    text(&format!("fromJSON(env.J){suffix}"), &env_with_json(json)?)
}

fn dump(json: &str) -> Result<String> {
    text("toJSON(fromJSON(env.J))", &env_with_json(json)?)
}

fn depth_error(json: &str) -> Result<String> {
    error_text("fromJSON(env.J)", &env_with_json(json)?)
}

fn max_depth(path: &str, line: usize, position: usize) -> String {
    format!(
        "The reader's MaxDepth of 64 has been exceeded. Path '{path}', line {line}, position {position}."
    )
}

#[test]
fn expr_from_json_replaces_exact_repeats_then_merges_case_insensitively() -> Result<()> {
    assert_eq!(read(r#"{"a":1,"a":2}"#, ".a")?, "2");
    assert_eq!(read(r#"{"a":1,"A":2}"#, ".a")?, "2");
    assert_eq!(dump(r#"{"a":1,"A":2}"#)?, "{\n  \"a\": 2\n}");
    assert_eq!(
        dump(r#"{"a":1,"b":3,"A":2}"#)?,
        "{\n  \"a\": 2,\n  \"b\": 3\n}"
    );
    assert_eq!(read(r#"[{"a":1,"a":2}]"#, "[0].a")?, "2");
    assert_eq!(dump(r#"{"a":1,"b":2}"#)?, "{\n  \"a\": 1,\n  \"b\": 2\n}");

    assert_eq!(dump(r#"{"A":1,"a":2,"A":3}"#)?, "{\n  \"A\": 2\n}");
    assert_eq!(dump(r#"{"a":1,"A":2,"a":3}"#)?, "{\n  \"a\": 2\n}");

    assert_eq!(
        dump("{\"\u{e4}\":1,\"\u{c4}\":2}")?,
        "{\n  \"\u{e4}\": 2\n}"
    );
    assert_eq!(
        dump("{\"s\":1,\"\u{17f}\":2}")?,
        "{\n  \"s\": 1,\n  \"\u{17f}\": 2\n}"
    );
    Ok(())
}

#[test]
fn expr_from_json_caps_nesting_at_64() -> Result<()> {
    let arrays = |depth: usize| format!("{}1{}", "[".repeat(depth - 1), "]".repeat(depth - 1));
    let objects = |depth: usize| format!("{}1{}", "{\"a\":".repeat(depth), "}".repeat(depth));
    assert!(dump(&arrays(64))?.contains('1'));
    assert!(dump(&objects(64))?.contains('1'));

    let array_path = "[0]".repeat(64);
    let object_path = vec!["a"; 64].join(".");
    for depth in [65, 128, 200] {
        assert_eq!(
            depth_error(&arrays(depth + 1))?,
            max_depth(&array_path, 1, 65),
            "{depth} arrays"
        );
        assert_eq!(
            depth_error(&objects(depth))?,
            max_depth(&object_path, 1, 321),
            "{depth} objects"
        );
    }
    Ok(())
}

#[test]
fn expr_from_json_reports_newtonsofts_path_and_position() -> Result<()> {
    let mixed = format!("{{\"a\":[1,{{\"b\":{}", "[".repeat(62));
    assert_eq!(
        depth_error(&mixed)?,
        max_depth(&format!("a[1].b{}", "[0]".repeat(61)), 1, 75)
    );

    let dotted = format!("{{\"x.y\":{}", "[".repeat(64));
    assert_eq!(
        depth_error(&dotted)?,
        max_depth(&format!("['x.y']{}", "[0]".repeat(63)), 1, 71)
    );
    let quoted = format!("{{\"it's\":{}", "[".repeat(64));
    assert_eq!(
        depth_error(&quoted)?,
        max_depth(&format!("['it\\'s']{}", "[0]".repeat(63)), 1, 72)
    );

    let deep = "[".repeat(65);
    let path = "[0]".repeat(64);
    assert_eq!(
        depth_error(&format!("\n\n  {deep}"))?,
        max_depth(&path, 3, 67)
    );
    assert_eq!(
        depth_error(&format!("\r\n{deep}"))?,
        max_depth(&path, 2, 65)
    );
    assert_eq!(depth_error(&format!("\r{deep}"))?, max_depth(&path, 2, 65));

    let astral = format!("[\"\u{1f600}\",{}", "[".repeat(64));
    assert_eq!(
        depth_error(&astral)?,
        max_depth(&format!("[1]{}", "[0]".repeat(63)), 1, 70)
    );
    Ok(())
}

#[test]
fn expr_from_json_reports_a_syntax_error_that_precedes_the_depth() -> Result<()> {
    let broken = format!("[1 2,{}", "[".repeat(70));
    assert!(depth_error(&broken)?.starts_with(PREFIX));
    assert!(error_text("fromJSON('[] [[')", &empty_env())?.starts_with(PREFIX));
    Ok(())
}
