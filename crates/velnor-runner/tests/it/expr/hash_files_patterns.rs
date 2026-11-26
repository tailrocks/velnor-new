// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;
use std::fs;
use velnor_runner::expr::ExprEnv;

#[test]
fn expr_hash_files_accepts_dot_slash_prefix() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('./a.txt')", &env)?,
        expected_digest(&[b"a"])
    );
    assert_eq!(
        text("hashFiles('./sub/*.txt')", &env)?,
        expected_digest(&[b"b"])
    );
    assert_eq!(
        text("hashFiles('./**/*.txt')", &env)?,
        expected_digest(&[b"a", b"b"])
    );
    assert_eq!(
        text("hashFiles('.')", &env)?,
        expected_digest(&[b"a", b"b"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_bang_after_dot_slash_names_a_file() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/!bang", "bang")?;
    dir.write("ws/a.txt", "a")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('./!bang')", &env)?,
        expected_digest(&[b"bang"])
    );
    assert_eq!(
        text("hashFiles('*', '!./!bang')", &env)?,
        expected_digest(&[b"a"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_directory_name_hashes_descendants() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/src/one.rs", "1")?;
    dir.write("ws/src/nested/two.rs", "2")?;
    dir.write("ws/other.rs", "o")?;
    let env = dir.env();
    let expected = expected_digest(&[b"2", b"1"]);

    for pattern in ["src", "src/", "src/**", "./src", "src/**/"] {
        assert_eq!(
            text(&format!("hashFiles('{pattern}')"), &env)?,
            expected,
            "{pattern}"
        );
    }
    Ok(())
}

#[test]
fn expr_hash_files_rejects_relative_pathing() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("outside.txt", "out")?;
    let env = dir.env();

    for pattern in [
        "..",
        "../outside.txt",
        "../*.txt",
        "a/../b",
        "a/./b",
        "!../x",
    ] {
        let message = error_text(&format!("hashFiles('{pattern}')"), &env)?;
        assert!(
            message.starts_with(&format!(
                "hashFiles('{pattern}') failed. Fail to hash files under directory '"
            )),
            "{message}"
        );
        assert!(
            message.contains("Relative pathing '.' and '..' is not allowed."),
            "{message}"
        );
    }
    assert!(error_text("hashFiles('!')", &env)?.contains("pattern cannot be empty"));
    Ok(())
}

#[test]
fn expr_hash_files_resolves_absolute_paths_inside_workspace() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    dir.write("other/c.txt", "c")?;
    let env = dir.env();
    let workspace = dir.workspace()?;
    let parent = fs::canonicalize(dir.path())?.display().to_string();

    assert_eq!(
        text(&format!("hashFiles('{}/a.txt')", workspace.display()), &env)?,
        expected_digest(&[b"a"])
    );
    assert_eq!(
        text(
            &format!("hashFiles('{}/**/*.txt')", workspace.display()),
            &env
        )?,
        expected_digest(&[b"a", b"b"])
    );
    assert_eq!(
        text(&format!("hashFiles('{}')", workspace.display()), &env)?,
        expected_digest(&[b"a", b"b"])
    );
    assert_eq!(
        text(&format!("hashFiles('{parent}/*/a.txt')"), &env)?,
        expected_digest(&[b"a"])
    );
    assert_eq!(
        text(&format!("hashFiles('{parent}/other/c.txt')"), &env)?,
        ""
    );
    assert_eq!(
        text("hashFiles('/')", &env)?,
        expected_digest(&[b"a", b"b"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_trims_and_skips_blank_and_comment_lines() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/b.txt", "b")?;
    dir.write("ws/#c.txt", "c")?;
    let env = dir.env();
    let expected = expected_digest(&[b"a", b"b"]);

    assert_eq!(text("hashFiles('  a.txt  ', ' b.txt')", &env)?, expected);
    assert_eq!(
        text("hashFiles('# note\n\n  a.txt\n   \n b.txt\r')", &env)?,
        expected
    );
    assert_eq!(text("hashFiles('#c.txt')", &env)?, "");
    assert_eq!(text("hashFiles('', '  ')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_treats_braces_and_open_brackets_literally() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/b.txt", "b")?;
    dir.write("ws/{a,b}.txt", "brace")?;
    dir.write("ws/x{1}", "one")?;
    dir.write("ws/[.txt", "open")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('{a,b}.txt')", &env)?,
        expected_digest(&[b"brace"])
    );
    assert_eq!(text("hashFiles('x{1}')", &env)?, expected_digest(&[b"one"]));
    assert_eq!(
        text("hashFiles('[.txt')", &env)?,
        expected_digest(&[b"open"])
    );
    assert_eq!(text("hashFiles('[')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_matches_generated_brace_keys() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a{b}.txt", "brace")?;
    dir.write("ws/ab.txt", "plain")?;
    let env = dir.env();
    let expression = r#"hashFiles(format('{0}{1}{2}{3}{4}', 'a', fromJSON('"{"'), 'b', fromJSON('"}"'), '.txt'))"#;

    assert_eq!(text(expression, &env)?, expected_digest(&[b"brace"]));
    Ok(())
}

#[test]
fn expr_hash_files_applies_patterns_in_order() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/b.txt", "b")?;
    dir.write("ws/c.txt", "c")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('*.txt', '!b.txt')", &env)?,
        expected_digest(&[b"a", b"c"])
    );
    assert_eq!(
        text("hashFiles('!b.txt', '*.txt')", &env)?,
        expected_digest(&[b"a", b"b", b"c"])
    );
    assert_eq!(
        text("hashFiles('*.txt', '!b.txt', 'b.txt')", &env)?,
        expected_digest(&[b"a", b"b", b"c"])
    );
    assert_eq!(text("hashFiles('!a.txt')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_negated_patterns_do_not_reorder_search_roots() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/b/1.txt", "b1")?;
    dir.write("ws/a/2.txt", "a2")?;
    dir.write("ws/c.md", "c")?;
    let env = dir.env();
    let expected = expected_digest(&[b"b1", b"a2"]);

    assert_eq!(text("hashFiles('b/*.txt', 'a/*.txt')", &env)?, expected);
    assert_eq!(
        text("hashFiles('b/*.txt', 'a/*.txt', '!*.md')", &env)?,
        expected
    );
    Ok(())
}

#[test]
fn expr_hash_files_trailing_slash_needs_a_descendant_segment() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/d/b.txt", "b")?;
    dir.write("ws/sub/x/y.txt", "y")?;
    dir.write("ws/sub/z.txt", "z")?;
    let env = dir.env();

    assert_eq!(
        text("hashFiles('*/')", &env)?,
        expected_digest(&[b"b", b"y", b"z"])
    );
    assert_ne!(
        text("hashFiles('*/')", &env)?,
        text("hashFiles('*')", &env)?
    );
    assert_eq!(text("hashFiles('sub/*/')", &env)?, expected_digest(&[b"y"]));
    assert_eq!(text("hashFiles('a.txt/')", &env)?, "");
    assert_eq!(
        text("hashFiles('**', '!*/')", &env)?,
        expected_digest(&[b"a"])
    );
    Ok(())
}

#[test]
fn expr_hash_files_expands_tilde_to_home_before_clamping_to_the_workspace() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    dir.write("ws/~x", "tilde")?;
    dir.write("other/c.txt", "c")?;
    let mut env = dir.env();
    env.home = Some(fs::canonicalize(dir.path())?);

    assert_eq!(
        text("hashFiles('~/ws/a.txt')", &env)?,
        expected_digest(&[b"a"])
    );
    assert_eq!(
        text("hashFiles('~/ws/**/*.txt')", &env)?,
        expected_digest(&[b"a", b"b"])
    );
    assert_eq!(text("hashFiles('~/other/c.txt')", &env)?, "");
    assert_eq!(text("hashFiles('~/a.txt')", &env)?, "");
    let everything = expected_digest(&[b"a", b"b", b"tilde"]);
    assert_eq!(text("hashFiles('~')", &env)?, everything);
    assert_eq!(text("hashFiles('~/')", &env)?, everything);
    assert_eq!(text("hashFiles('~x')", &env)?, expected_digest(&[b"tilde"]));
    Ok(())
}

#[test]
fn expr_hash_files_tilde_needs_a_rooted_home() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/~x", "tilde")?;
    let mut env = dir.env();

    env.home = None;
    assert!(
        error_text("hashFiles('~/a.txt')", &env)?.contains("Unable to determine HOME directory")
    );
    env.home = Some("relative/home".into());
    assert!(
        error_text("hashFiles('~')", &env)?
            .contains("Expected HOME directory to be a rooted path. Actual 'relative/home'")
    );
    assert_eq!(text("hashFiles('~x')", &env)?, expected_digest(&[b"tilde"]));
    Ok(())
}

#[test]
fn expr_hash_files_does_not_canonicalize_home() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    std::os::unix::fs::symlink(dir.path(), dir.path().join("link"))?;
    let mut env = dir.env();
    env.home = Some(dir.path().join("link"));
    assert_eq!(text("hashFiles('~/ws/a.txt')", &env)?, "");
    Ok(())
}

#[test]
fn expr_hash_files_rejects_relative_home_whatever_the_cwd() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let mut env = dir.env();
    env.home = Some("src".into());
    assert!(
        error_text("hashFiles('~')", &env)?
            .contains("Expected HOME directory to be a rooted path. Actual 'src'")
    );
    Ok(())
}

#[test]
fn expr_hash_files_rejects_empty_home_var() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    let env = ExprEnv::with_home_var(dir.env().workspace, Some(String::new().into()));
    assert!(error_text("hashFiles('~')", &env)?.contains("Unable to determine HOME directory"));
    Ok(())
}

#[test]
fn expr_hash_files_rejects_dotted_home() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    let root = fs::canonicalize(dir.path())?;
    let mut env = dir.env();

    for home in [root.join("ws/sub/.."), root.join(".").join("ws")] {
        env.home = Some(home);
        for pattern in ["~/a.txt", "~/**"] {
            let message = error_text(&format!("hashFiles('{pattern}')"), &env)?;
            assert!(
                message.contains("Relative pathing '.' and '..' is not allowed."),
                "{pattern}: {message}"
            );
        }
    }
    Ok(())
}
