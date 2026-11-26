// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use crate::expr_support::{TempDir, error_text, text};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use velnor_runner::expr::ExprEnv;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn expected_digest(contents: &[&[u8]]) -> String {
    let mut aggregate = Sha256::new();
    for content in contents {
        aggregate.update(Sha256::digest(content));
    }
    hex(&aggregate.finalize())
}

fn workspace_env(dir: &TempDir) -> ExprEnv {
    ExprEnv::new(dir.path().join("ws"))
}

#[test]
fn expr_hash_files_matches_github_digest() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a\n")?;
    dir.write("ws/b.txt", "b\n")?;
    dir.write("ws/c.md", "c\n")?;
    let env = workspace_env(&dir);

    assert_eq!(
        text("hashFiles('*.txt')", &env)?,
        expected_digest(&[b"a\n", b"b\n"])
    );
    assert_eq!(
        text("hashFiles('a.txt', 'b.txt')", &env)?,
        expected_digest(&[b"a\n", b"b\n"])
    );
    assert_eq!(text("hashFiles('nomatch')", &env)?, "");
    assert_eq!(text("hashFiles('*.rs')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_walks_directories_and_honors_negation() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/src/one.rs", "1")?;
    dir.write("ws/src/nested/two.rs", "2")?;
    dir.write("ws/src/nested/skip.rs", "x")?;
    dir.write("ws/other.rs", "o")?;
    let env = workspace_env(&dir);

    assert_eq!(
        text("hashFiles('src/**/*.rs', '!**/skip.rs')", &env)?,
        expected_digest(&[b"2", b"1"])
    );
    assert_eq!(
        text("hashFiles('**/one.rs')", &env)?,
        expected_digest(&[b"1"])
    );
    assert_eq!(text("hashFiles('src')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_ignores_paths_outside_workspace() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/inside.txt", "in")?;
    dir.write("outside.txt", "out")?;
    let env = workspace_env(&dir);

    assert_eq!(text("hashFiles('../outside.txt')", &env)?, "");
    assert_eq!(text("hashFiles('../*.txt')", &env)?, "");
    let outside = dir.path().join("outside.txt");
    let absolute = format!("hashFiles('{}')", outside.display());
    assert_eq!(text(&absolute, &env)?, "");
    assert_eq!(text("hashFiles('*.txt')", &env)?, expected_digest(&[b"in"]));
    Ok(())
}

#[test]
fn expr_hash_files_rejects_unknown_option_and_bad_glob() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let env = workspace_env(&dir);

    assert!(
        error_text("hashFiles('--follow-symbolic-links', '*.txt')", &env)?
            .contains("Invalid glob option --follow-symbolic-links")
    );
    assert!(error_text("hashFiles('[')", &env)?.contains("Invalid glob pattern"));
    assert!(error_text("hashFiles()", &env)?.contains("Too few parameters"));
    Ok(())
}
