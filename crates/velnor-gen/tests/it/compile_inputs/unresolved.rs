// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::embed::{selected, unresolved, view_of};
use crate::facts::TestResult;
use crate::select::path;

const MEMBERS: [&str; 4] = ["app", "c", "tool", "left"];

fn through_symlink(source: &str, link: &str, selects: bool) -> TestResult {
    let mut view = view_of(
        &MEMBERS,
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
        ],
    )?;
    view.tracked_paths.insert(path(link));
    view.symlinks.insert(path(link));
    assert_eq!(
        selected(&view, "docs/other.md")?,
        if selects {
            vec![("c".to_owned(), "unresolved-include".to_owned())]
        } else {
            Vec::new()
        },
        "{source} with {link} a symlink"
    );
    Ok(())
}

#[test]
fn an_include_through_a_tracked_symlink_is_unresolved() -> TestResult {
    for source in [
        "const A: &str = include_str!(\"../assets/sub/guide.md\");",
        "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/sub/guide.md\"));",
        "const A: &str = include_str!(\"../assets\");",
    ] {
        through_symlink(source, "crates/c/assets", true)?;
        through_symlink(source, "crates/c/other", false)?;
    }
    Ok(())
}

#[test]
fn a_backslash_in_any_literal_makes_the_include_unresolved() -> TestResult {
    for source in [
        "include_str!(\"..\\\\x.md\");",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src\\\\x.md\"));",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src\", \"\\\\x.md\"));",
        "include_str!(concat!(env!(\"OUT_DIR\"), \"/\\x2e\\x2e/x.md\"));",
        "include_str!(concat!(env!(\"OUT_DIR\"), r\"\\..\\x.md\"));",
        "include_str!(concat!(env!(\"OUT_DIR\"), \"/a\\\n  /x.md\"));",
    ] {
        unresolved(source)?;
    }
    Ok(())
}

#[test]
fn a_file_that_defines_or_imports_a_macro_named_like_a_builtin_marks_its_crate_unresolved()
-> TestResult {
    let out = "const A: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/g.rs\"));\nconst B: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/b\"));\ninclude!(concat!(env!(\"OUT_DIR\"), \"/c.rs\"));";
    for shadow in [
        "macro_rules! env { () => { \"/repo\" }; }",
        "macro_rules! concat { ($($t:tt)*) => { \"x\" }; }",
        "macro_rules! include_str { ($p:expr) => { \"\" }; }",
        "macro env() {}",
        "use helpers::env;",
        "use helpers::{concat as c, include_bytes};",
        "use helpers::util as include;",
        "use std::{env, fs};",
    ] {
        unresolved(&format!("{shadow}\n{out}"))?;
    }
    Ok(())
}
