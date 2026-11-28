// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, lock, text};
use anyhow::Result;
use std::fs;
use std::os::unix::net::UnixListener;
use uuid::Uuid;

fn place_socket(dir: &TempDir, relative: &str) -> Result<UnixListener> {
    let short =
        std::env::temp_dir().join(format!("s{}", &Uuid::new_v4().simple().to_string()[..12]));
    let listener = UnixListener::bind(&short)?;
    fs::rename(&short, dir.path().join(relative))?;
    Ok(listener)
}

#[test]
fn expr_hash_files_passes_through_a_socket_that_only_a_prefix_reaches() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let _listener = place_socket(&dir, "ws/sock")?;
    let env = dir.env();

    for pattern in ["sock/*", "sock/**/*.txt"] {
        assert_eq!(text(&format!("hashFiles('{pattern}')"), &env)?, "");
    }
    assert_eq!(
        text("hashFiles('a.txt', 'sock/*')", &env)?,
        expected_digest(&[b"a"])
    );
    let message = error_text("hashFiles('sock')", &env)?;
    assert!(message.ends_with("sock: not a regular file"), "{message}");
    Ok(())
}

#[test]
fn expr_hash_files_passes_through_an_unreadable_file_that_only_a_prefix_reaches() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sealed.md", "s")?;
    let sealed = dir.workspace()?.join("sealed.md");
    let _file = lock(&sealed)?;
    let env = dir.env();

    for pattern in ["sealed.md/*", "sealed.md/**/*.txt"] {
        assert_eq!(text(&format!("hashFiles('{pattern}')"), &env)?, "");
    }
    assert_eq!(
        text("hashFiles('a.txt', 'sealed.md/*')", &env)?,
        expected_digest(&[b"a"])
    );
    let message = error_text("hashFiles('sealed.md')", &env)?;
    assert!(message.contains("sealed.md"), "{message}");
    Ok(())
}
