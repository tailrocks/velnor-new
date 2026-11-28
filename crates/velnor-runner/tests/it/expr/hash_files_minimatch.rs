// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;

fn hashes_only(pattern: &str, files: &[&str], matching: &[&str]) -> Result<()> {
    let dir = TempDir::new()?;
    for file in files {
        dir.write(&format!("ws/{file}"), file)?;
    }
    let contents: Vec<&[u8]> = matching.iter().map(|name| name.as_bytes()).collect();
    let expected = if contents.is_empty() {
        String::new()
    } else {
        expected_digest(&contents)
    };
    assert_eq!(
        text(&format!("hashFiles('{pattern}')"), &dir.env())?,
        expected,
        "{pattern:?} over {files:?} should hash exactly {matching:?}"
    );
    Ok(())
}

#[test]
fn expr_hash_files_translates_classes_the_way_minimatch_does() -> Result<()> {
    hashes_only("[a-c-e]é", &["aé", "-é", "dé", "é"], &["-é", "aé"])?;
    hashes_only("*[a\\-c]*", &["b", "-", "d"], &["b"])?;
    hashes_only("[\\]]?", &["]x", "x]", "]"], &["]x"])?;
    hashes_only("*[!]", &["a", "ab", "\nx"], &["a", "ab"])?;
    hashes_only(
        "[a-c-e]*",
        &["-x", "bx", "dx", "ex", "é"],
        &["-x", "bx", "ex"],
    )?;
    hashes_only("*[]]", &["a]", "a"], &["a]"])?;
    hashes_only("*[]a]", &["x]", "xa", "xb"], &["x]", "xa"])?;
    Ok(())
}

#[test]
fn expr_hash_files_reproduces_minimatch_fallbacks_for_broken_classes() -> Result<()> {
    hashes_only("[z-a]", &["[z-a]", "z"], &["[z-a]"])?;
    hashes_only("[z\\-a]", &["[z-a]", "z", "-"], &[])?;
    hashes_only("*[z-a]", &["x[z-a]", "x"], &["x[z-a]"])?;
    hashes_only("*[", &["x[", "x[undefined"], &["x[undefined"])?;
    hashes_only("a[", &["a[", "a"], &["a["])?;
    Ok(())
}

#[test]
fn expr_hash_files_judges_a_class_by_the_text_up_to_the_first_bracket_the_regexp_leaves_open()
-> Result<()> {
    hashes_only("[a\\@])]", &["a", "[a@])]", "@", "]"], &["[a@])]"])?;
    hashes_only(
        "*/[\\@]z-a]",
        &["d/a", "d/@z-a]", "d/[@]z-a]", "d/[\\@]z-a]", "d/z-a]"],
        &[],
    )?;
    hashes_only("[\\@]**]", &["@", "]", "*", "[@]**]", "@x]"], &["[@]**]"])?;
    Ok(())
}

#[test]
fn expr_hash_files_judges_a_class_that_starts_with_a_bracket_as_a_whole_regular_expression()
-> Result<()> {
    hashes_only("*[](]", &["x[](]", "x]", "x(", "x", "x]("], &["x[](]"])?;
    hashes_only(
        "*[]a|b]",
        &["xa", "x]", "xb", "x|", "x", "x[]a|b]"],
        &["x[]a|b]", "x]", "xa", "xb", "x|"],
    )?;
    Ok(())
}

#[test]
fn expr_hash_files_refuses_a_class_that_holds_a_named_or_modified_regular_expression_group()
-> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/xa", "xa")?;
    for pattern in ["[](?<n>a)]", "[\\@](?i:a)]", "[\\@](?<é>a)]"] {
        let message = error_text(&format!("hashFiles('{pattern}')"), &dir.env())?;
        assert!(
            message.ends_with(&format!(
                "pattern segment '{pattern}' holds a named or modified regular expression group inside a character class, which is not supported"
            )),
            "{message}"
        );
    }
    assert_eq!(
        text("hashFiles('*[]a]')", &dir.env())?,
        expected_digest(&[b"xa"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_keeps_the_lookahead_minimatch_puts_after_an_unclosed_class() -> Result<()> {
    hashes_only("foo[*", &["foo[", "foo[x", "foo[\nx"], &["foo[x"])?;
    hashes_only("x[?", &["x[", "x[a", "x[\n"], &["x[a"])?;
    hashes_only("[😀?", &["[😀", "[😀\n", "[😀a"], &["[😀\n", "[😀a"])?;
    hashes_only("*[*", &["[", "a[", "a[b", "a[\n"], &["a[b"])?;
    Ok(())
}

#[test]
fn expr_hash_files_starts_a_search_at_a_leading_one_character_class() -> Result<()> {
    hashes_only("[!]", &["!", "a", "b"], &["!"])?;
    hashes_only("[^]", &["^", "a", "b"], &["^"])?;
    hashes_only("[]]", &["]", "a"], &[])?;
    Ok(())
}

#[test]
fn expr_hash_files_doubles_the_trailing_backslash_after_an_unclosed_class() -> Result<()> {
    hashes_only("*[\\", &["a[\\", "a[\\\\", "a["], &["a[\\\\"])?;
    hashes_only("a[\\", &["a[\\", "a[\\\\"], &["a[\\"])?;
    Ok(())
}

#[test]
fn expr_hash_files_puts_the_lookahead_before_each_wildcard_run_after_an_unclosed_class()
-> Result<()> {
    hashes_only(
        "[*b[",
        &[
            "[xb[undefined",
            "[b[undefined",
            "[\nb[undefined",
            "[xb[",
            "xb[undefined",
        ],
        &["[b[undefined", "[xb[undefined"],
    )
}
