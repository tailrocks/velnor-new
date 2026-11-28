// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, expected_digest, hash_outcome, text};
use anyhow::Result;

#[test]
fn expr_hash_files_matches_wildcards_by_utf16_code_unit() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/é.txt", "e")?;
    dir.write("ws/😀.txt", "s")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('?.txt')", &env)?,
        expected_digest(&[b"a", b"e"])
    );
    assert_eq!(text("hashFiles('??.txt')", &env)?, expected_digest(&[b"s"]));
    assert_eq!(
        text("hashFiles('[!b].txt')", &env)?,
        expected_digest(&[b"a", b"e"])
    );
    assert_eq!(
        text("hashFiles('*?.txt')", &env)?,
        expected_digest(&[b"a", b"e", b"s"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_matches_non_ascii_classes() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/é.txt", "e")?;
    dir.write("ws/ü.txt", "u")?;
    dir.write("ws/ý.txt", "y")?;
    dir.write("ws/Cargo.lock", "c")?;
    dir.write("ws/docs/日本語.md", "j")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('[éa].txt')", &env)?,
        expected_digest(&[b"a", b"e"])
    );
    assert_eq!(
        text("hashFiles('[é-ü].txt')", &env)?,
        expected_digest(&[b"e", b"u"])
    );
    assert_eq!(
        text("hashFiles('**/[Cc]argo.lock')", &env)?,
        expected_digest(&[b"c"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_matches_literal_and_star_patterns_against_non_ascii_names() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/é.txt", "e")?;
    let env = dir.env();

    assert_eq!(text("hashFiles('é.txt')", &env)?, expected_digest(&[b"e"]));
    assert_eq!(
        text("hashFiles('*.txt')", &env)?,
        expected_digest(&[b"a", b"e"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_reads_a_class_of_one_astral_character_as_two_units() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let mut env = dir.env();

    assert_eq!(hash_outcome(&mut env, "a.txt/[😀]/x")?, "ok: ");
    let one_unit = hash_outcome(&mut env, "a.txt/[b]/x")?;
    assert!(one_unit.contains("Not a directory"), "{one_unit}");
    Ok(())
}
