// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;

#[test]
fn expr_hash_files_refuses_wildcards_that_would_split_a_non_ascii_name() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/é.txt", "e")?;
    dir.write("ws/sub/b.txt", "b")?;
    let env = dir.env();

    for pattern in ["?.txt", "[!b].txt", "[a-z].txt"] {
        let message = error_text(&format!("hashFiles('{pattern}')"), &env)?;
        assert!(
            message.starts_with(&format!("hashFiles('{pattern}') failed.")),
            "{message}"
        );
        assert!(
            message.ends_with(&format!(
                "pattern segment '{pattern}' uses '?' or a character class, which is not supported for the non-ASCII name 'é.txt'"
            )),
            "{message}"
        );
    }
    assert_eq!(
        text("hashFiles('sub/?.txt')", &env)?,
        expected_digest(&[b"b"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_refuses_character_classes_with_non_ascii_characters() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let env = dir.env();

    let message = error_text("hashFiles('[éa].txt')", &env)?;
    assert!(
        message.ends_with(
            "pattern segment '[éa].txt' has a character class with a non-ASCII character, which is not supported"
        ),
        "{message}"
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
