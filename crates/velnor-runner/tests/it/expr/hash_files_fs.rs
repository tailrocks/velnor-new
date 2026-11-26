// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

struct Restore<'a>(&'a Path);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o755));
    }
}

fn lock(path: &Path) -> Result<Option<Restore<'_>>> {
    let guard = Restore(path);
    fs::set_permissions(path, fs::Permissions::from_mode(0o000))?;
    let still_readable = fs::read_dir(path).is_ok() || fs::File::open(path).is_ok();
    Ok((!still_readable).then_some(guard))
}

#[test]
fn expr_hash_files_ignores_symlinks_that_leave_the_workspace() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("outside/secret.txt", "secret")?;
    dir.symlink("ws/link.txt", &dir.path().join("outside/secret.txt"))?;
    dir.symlink("ws/linkdir", &dir.path().join("outside"))?;
    let env = dir.env();
    let expected = expected_digest(&[b"a"]);

    assert_eq!(text("hashFiles('*.txt')", &env)?, expected);
    assert_eq!(text("hashFiles('**/*.txt')", &env)?, expected);
    assert_eq!(text("hashFiles('**')", &env)?, expected);
    assert_eq!(text("hashFiles('linkdir/*.txt')", &env)?, "");
    assert_eq!(text("hashFiles('linkdir/**')", &env)?, "");
    assert_eq!(text("hashFiles('linkdir')", &env)?, "");
    assert_eq!(text("hashFiles('link.txt')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_reads_file_symlinks_that_stay_inside_the_workspace() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/real/x.txt", "x")?;
    dir.write("ws/real/sub/y.txt", "y")?;
    dir.write("ws/a.txt", "a")?;
    dir.symlink("ws/alias.txt", &dir.workspace()?.join("real/x.txt"))?;
    dir.symlink("ws/dirlink", &dir.workspace()?.join("real"))?;
    dir.symlink("ws/dangling.txt", &dir.workspace()?.join("missing.txt"))?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('**/*.txt')", &env)?,
        expected_digest(&[b"a", b"x", b"y", b"x"])
    );
    assert_eq!(text("hashFiles('dirlink/*.txt')", &env)?, "");
    assert_eq!(
        text("hashFiles('dirlink/sub/*.txt')", &env)?,
        expected_digest(&[b"y"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_reports_unreadable_directories_and_files() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/locked/x.txt", "x")?;
    dir.write("ws/sealed.md", "s")?;
    let env = dir.env();
    let workspace = dir.workspace()?;

    let locked = workspace.join("locked");
    let Some(_directory) = lock(&locked)? else {
        return Ok(());
    };
    let message = error_text("hashFiles('**/*.txt')", &env)?;
    assert!(
        message.starts_with("hashFiles('**/*.txt') failed. Fail to hash files under directory '"),
        "{message}"
    );
    assert!(message.contains("locked"), "{message}");

    let sealed = workspace.join("sealed.md");
    let Some(_file) = lock(&sealed)? else {
        return Ok(());
    };
    let message = error_text("hashFiles('*.md')", &env)?;
    assert!(message.contains("sealed.md"), "{message}");
    Ok(())
}

#[test]
fn expr_hash_files_does_not_enter_directories_no_pattern_can_match() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/target/deep/x.txt", "x")?;
    dir.write("ws/src/b.txt", "b")?;
    let env = dir.env();
    let workspace = dir.workspace()?;

    let target = workspace.join("target");
    let Some(_target) = lock(&target)? else {
        return Ok(());
    };
    assert_eq!(text("hashFiles('*.txt')", &env)?, expected_digest(&[b"a"]));
    assert_eq!(
        text("hashFiles('src/*.txt', '!target/**')", &env)?,
        expected_digest(&[b"b"])
    );
    assert!(error_text("hashFiles('**/*.txt')", &env)?.contains("target"));
    Ok(())
}

#[test]
fn expr_hash_files_walks_many_siblings_and_deep_trees() -> Result<()> {
    let dir = TempDir::new()?;
    let nested = (0..60).fold(String::from("ws"), |path, level| format!("{path}/d{level}"));
    dir.write(&format!("{nested}/leaf.txt"), "leaf")?;
    for index in 0..1500 {
        dir.write(&format!("ws/many/f{index:04}.txt"), "m")?;
    }
    let env = dir.env();

    assert_eq!(
        text("hashFiles('d0/**/leaf.txt')", &env)?,
        expected_digest(&[b"leaf"])
    );
    let many = vec![b"m".as_slice(); 1500];
    assert_eq!(
        text("hashFiles('many/*.txt')", &env)?,
        expected_digest(&many)
    );
    Ok(())
}
