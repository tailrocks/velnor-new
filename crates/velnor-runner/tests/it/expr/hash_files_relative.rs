// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;

const REFUSAL: &str = "Relative pathing '.' and '..' is not allowed.";

fn workspace() -> Result<TempDir> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    Ok(dir)
}

#[test]
fn expr_hash_files_refuses_a_hidden_dot_when_the_runner_adds_descendants() -> Result<()> {
    let dir = workspace()?;
    let env = dir.env();
    let root = dir.workspace()?.display().to_string();

    for (pattern, rebuilt) in [
        ("[.]", "[.]/**"),
        ("[.]/a.txt", "[.]/a.txt/**"),
        ("\\./a.txt", "\\./a.txt/**"),
        ("[.]/", "[.]/**"),
        ("[\\.]", "[\\.]/**"),
        ("[.]/**/*.txt", "[.]/**/*.txt/**"),
        ("[.]/**/", "[.]/**/**"),
    ] {
        let message = error_text(&format!("hashFiles('{pattern}')"), &env)?;
        assert!(
            message.ends_with(&format!("Invalid pattern '{root}/{rebuilt}'. {REFUSAL}")),
            "{pattern}: {message}"
        );
    }
    Ok(())
}

#[test]
fn expr_hash_files_hashes_nothing_for_a_hidden_dot_before_a_final_globstar() -> Result<()> {
    let dir = workspace()?;
    let env = dir.env();

    for pattern in ["[.]/**", "\\./**"] {
        assert_eq!(
            text(&format!("hashFiles('{pattern}')"), &env)?,
            "",
            "{pattern}"
        );
    }
    assert_eq!(
        text("hashFiles('a.txt', '[.]/**')", &env)?,
        expected_digest(&[b"a"])
    );
    Ok(())
}
