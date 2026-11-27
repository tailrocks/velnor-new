// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const MACOS_EILSEQ: i32 = 92;

fn write_named(directory: &Path, name: &[u8], contents: &str) -> Result<bool> {
    fs::create_dir_all(directory)?;
    match fs::write(directory.join(OsStr::from_bytes(name)), contents) {
        Ok(()) => Ok(true),
        Err(error) => {
            assert!(
                refused_by_the_filesystem(&error),
                "creating {name:?} failed for a reason other than the filesystem refusing the name: {error}"
            );
            Ok(false)
        }
    }
}

fn refused_by_the_filesystem(error: &io::Error) -> bool {
    cfg!(target_os = "macos") && error.raw_os_error() == Some(MACOS_EILSEQ)
}

#[test]
fn expr_hash_files_fails_when_the_walk_reaches_a_name_that_is_not_valid_utf8() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/dir2/ok.txt", "ok")?;
    let workspace = dir.workspace()?;
    if !write_named(&workspace.join("dir"), b"bad\xff.txt", "bad")? {
        return Ok(());
    }
    let env = dir.env();

    for expression in [
        "hashFiles('**')",
        "hashFiles('dir/*.txt')",
        "hashFiles('dir/**')",
    ] {
        let message = error_text(expression, &env)?;
        assert!(
            message.contains("failed. Fail to hash files under directory '"),
            "{message}"
        );
        assert!(message.contains("bad\u{FFFD}.txt"), "{message}");
        assert!(message.contains("No such file or directory"), "{message}");
    }
    assert_eq!(text("hashFiles('a.txt')", &env)?, expected_digest(&[b"a"]));
    for expression in [
        "hashFiles('dir2/*.txt')",
        "hashFiles('d*/ok.txt')",
        "hashFiles('*/ok.txt')",
    ] {
        assert_eq!(text(expression, &env)?, expected_digest(&[b"ok"]));
    }
    Ok(())
}

#[test]
fn expr_hash_files_reads_a_name_that_is_not_valid_utf8_as_its_replacement_character_twin()
-> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let both = dir.workspace()?.join("both");
    if !write_named(&both, b"a\xff.txt", "X")? {
        return Ok(());
    }
    write_named(&both, "a\u{FFFD}.txt".as_bytes(), "Y")?;
    let env = dir.env();

    let twice = expected_digest(&[b"Y", b"Y"]);
    assert_eq!(text("hashFiles('both/**')", &env)?, twice);
    assert_eq!(text("hashFiles('both/*.txt')", &env)?, twice);
    Ok(())
}
