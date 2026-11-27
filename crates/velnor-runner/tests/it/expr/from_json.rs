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
    let arrays = |depth: usize| format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let objects = |depth: usize| format!("{}1{}", "{\"a\":".repeat(depth), "}".repeat(depth));
    assert!(dump(&arrays(64))?.contains('1'));
    assert!(dump(&objects(64))?.contains('1'));

    let array_path = "[0]".repeat(64);
    let object_path = vec!["a"; 64].join(".");
    for depth in [65, 128, 200] {
        assert_eq!(
            depth_error(&arrays(depth))?,
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
fn merged_lookup(count: usize, key: impl Fn(usize) -> String, lookup: &str) -> Result<String> {
    let body = (0..count)
        .map(|index| format!("\"{}\":{index}", key(index)))
        .collect::<Vec<_>>()
        .join(",");
    let env = env_with_json(&format!("{{{body}}}"))?;
    let started = std::time::Instant::now();
    let value = text(&format!("fromJSON(env.J).{lookup}"), &env)?;
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "{count} keys took {elapsed:?}"
    );
    Ok(value)
}

#[test]
fn expr_from_json_merges_thousands_of_keys_in_linear_time() -> Result<()> {
    assert_eq!(merged_lookup(8_000, |i| format!("k{i}"), "k3")?, "3");
    assert_eq!(
        merged_lookup(8_000, |i| format!("k{}", i % 4_000), "k3")?,
        "4003"
    );
    assert_eq!(
        merged_lookup(
            20_000,
            |i| format!("{}{}", if i % 2 == 0 { 'k' } else { 'K' }, i / 2),
            "k3",
        )?,
        "7"
    );
    Ok(())
}

#[test]
fn expr_from_json_reads_numbers_beyond_the_double_range_as_infinity() -> Result<()> {
    assert_eq!(read("1e400", "")?, "Infinity");
    assert_eq!(read("-1e400", "")?, "-Infinity");
    assert_eq!(read("2e308", "")?, "Infinity");
    assert_eq!(read(&"9".repeat(400), "")?, "Infinity");
    assert_eq!(read("[1,1e400,-1e400,2]", "[1]")?, "Infinity");
    assert_eq!(read("[1,1e400,-1e400,2]", "[2]")?, "-Infinity");
    assert_eq!(read("[1,1e400,-1e400,2]", "[3]")?, "2");
    assert_eq!(
        read(r#"{"a":1e400,"b":{"c":-1.5E+999}}"#, ".b.c")?,
        "-Infinity"
    );
    assert_eq!(read(r#"{"a":1,"a":1e400}"#, ".a")?, "Infinity");
    assert_eq!(read(r#"{"a":1e400,"A":2}"#, ".a")?, "2");
    assert_eq!(read(r#"{"a":1e400, "b" : 3}"#, ".b")?, "3");

    assert_eq!(read("1e308", " > 0")?, "true");
    assert_eq!(read(r#""1e400""#, "")?, "1e400");
    assert_eq!(read(r#"["a\"1e400",3]"#, "[0]")?, "a\"1e400");
    assert!(error_text("fromJSON('01e400')", &empty_env())?.starts_with(PREFIX));
    assert!(error_text("fromJSON('1.e400')", &empty_env())?.starts_with(PREFIX));
    assert!(error_text("fromJSON('-1E+')", &empty_env())?.starts_with(PREFIX));
    assert_eq!(
        depth_error("[1e400 2]")?,
        format!("{PREFIX} expected `,` or `]` at line 1 column 8")
    );
    Ok(())
}

fn path_of(key_json: &str) -> Result<String> {
    let json = format!("{{{key_json}:{}", "[".repeat(64));
    let message = depth_error(&json)?;
    let path = message
        .split_once(" Path '")
        .and_then(|(_, rest)| rest.rsplit_once("', line"))
        .map(|(path, _)| path.to_string());
    path.ok_or_else(|| anyhow::anyhow!("no path in {message}"))
        .map(|path| path.trim_end_matches("[0]").to_string())
}

#[test]
fn expr_from_json_escapes_key_names_in_the_path_like_newtonsoft() -> Result<()> {
    assert_eq!(path_of("\"a\\u002eb\"")?, "['a.b']");
    assert_eq!(path_of("\"a\\u0062c\"")?, "abc");
    assert_eq!(path_of("\"a\\/b\"")?, "['a/b']");
    assert_eq!(path_of("\"a\\\"b\"")?, "['a\"b']");
    assert_eq!(path_of("\"\\ud83d\\ude00\"")?, "\u{1f600}");
    assert_eq!(
        path_of("\"a\\tb\\nc\\rd\\fe\\bf\\\\g\\u0085\\u2028\\u2029\\u0001\"")?,
        "['a\\tb\\nc\\rd\\fe\\bf\\\\g\\u0085\\u2028\\u2029\\u0001']"
    );
    Ok(())
}

#[test]
fn expr_from_json_tracks_the_path_through_scalars_and_siblings() -> Result<()> {
    let scalars = "[1,true,null,2.5,\"s\",{\"a\":1},[2],";
    assert_eq!(
        depth_error(&format!("{scalars}{}", "[".repeat(64)))?,
        max_depth(&format!("[7]{}", "[0]".repeat(63)), 1, scalars.len() + 64)
    );
    let siblings = "{\"x\":1,\"y\":\"s\",\"z\":{\"c\":\"d\",\"e\":";
    assert_eq!(
        depth_error(&format!("{siblings}{}", "[".repeat(63)))?,
        max_depth(&format!("z.e{}", "[0]".repeat(62)), 1, siblings.len() + 63)
    );
    Ok(())
}
