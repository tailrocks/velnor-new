// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoPath, RepoView, TargetKind};

use super::build_output::{DECLARES, with_build_script};
use super::embed::{selected, unresolved, view_of};
use crate::facts::{TestResult, lib, package, repo};
use crate::select::path;

const MEMBERS: [&str; 4] = ["app", "c", "tool", "left"];

const LINKED: [&str; 3] = [
    "const A: &str = include_str!(\"../assets/sub/guide.md\");",
    "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/sub/guide.md\"));",
    "const A: &str = include_str!(\"../assets\");",
];

fn through_link(
    source: &str,
    link: &str,
    selects: bool,
    links: fn(&mut RepoView) -> &mut BTreeSet<RepoPath>,
) -> TestResult {
    let mut view = view_of(
        &MEMBERS,
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
            ("crates/c/assets/sub/guide.md", ""),
        ],
    )?;
    view.tracked_paths.insert(path("crates/c/assets"));
    view.tracked_paths.insert(path(link));
    links(&mut view).insert(path(link));
    assert_eq!(
        selected(&view, "docs/other.md")?,
        if selects {
            vec![("c".to_owned(), "unresolved-include".to_owned())]
        } else {
            Vec::new()
        },
        "{source} with {link} linked"
    );
    Ok(())
}

#[test]
fn an_include_through_a_tracked_symlink_is_unresolved() -> TestResult {
    for source in LINKED {
        through_link(source, "crates/c/assets", true, |view| &mut view.symlinks)?;
        through_link(source, "crates/c/other", false, |view| &mut view.symlinks)?;
    }
    Ok(())
}

#[test]
fn an_include_through_a_submodule_is_unresolved() -> TestResult {
    for source in LINKED {
        through_link(source, "crates/c/assets", true, |view| &mut view.gitlinks)?;
        through_link(source, "crates/c/other", false, |view| &mut view.gitlinks)?;
    }
    Ok(())
}

fn generated(source: &str, tracked: bool) -> TestResult {
    let mut view = view_of(
        &MEMBERS,
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
        ],
    )?;
    if tracked {
        view.tracked_paths.insert(path("crates/c/generated.txt"));
    }
    assert_eq!(
        selected(&view, "docs/other.md")?,
        if tracked {
            Vec::new()
        } else {
            vec![("c".to_owned(), "unresolved-include".to_owned())]
        },
        "{source} with its target tracked: {tracked}"
    );
    Ok(())
}

#[test]
fn an_include_of_an_untracked_file_is_unresolved() -> TestResult {
    for source in [
        "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/generated.txt\"));",
        "const A: &str = include_str!(\"../generated.txt\");",
    ] {
        generated(source, false)?;
        generated(source, true)?;
    }
    Ok(())
}

#[test]
fn an_include_inside_a_macro_body_is_unresolved() -> TestResult {
    for source in [
        "macro_rules! m { () => { include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/x.txt\")) }; }",
        "macro_rules! m { () => { include_str!(\"data.txt\") }; }",
        "macro_rules! m { () => { include!(\"inc.rs\"); }; }",
        "macro_rules! m ( () => ( include_str!(\"data.txt\") ); );",
        "macro_rules! m { () => { macro_rules! n { () => { include_str!(\"d.txt\") }; } }; }",
        "macro_rules! m { () => { include_bytes!(concat!(env!(\"OUT_DIR\"), \"/g\")) }; }",
        "macro m() { include_str!(\"data.txt\") }",
        "macro m($x: expr) { include_str!(\"data.txt\") }",
    ] {
        let mut view = view_of(
            &MEMBERS,
            &[
                ("crates/c/src/lib.rs", source),
                ("crates/app/src/lib.rs", ""),
                ("crates/c/x.txt", ""),
                ("crates/c/src/data.txt", ""),
                ("crates/c/src/inc.rs", ""),
                ("crates/c/src/d.txt", ""),
            ],
        )?;
        with_build_script(&mut view, "c", DECLARES)?;
        assert_eq!(
            selected(&view, "docs/other.md")?,
            [("c".to_owned(), "unresolved-include".to_owned())],
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn a_form_after_a_macro_body_stays_resolved() -> TestResult {
    let view = view_of(
        &MEMBERS,
        &[
            (
                "crates/c/src/lib.rs",
                "macro_rules! m { () => { 1 }; }\nmacro n($x: expr) { 2 }\nconst A: &str = include_str!(\"../../../docs/guide.md\");",
            ),
            ("crates/app/src/lib.rs", ""),
            ("docs/guide.md", ""),
        ],
    )?;
    assert_eq!(selected(&view, "docs/other.md")?, Vec::new());
    assert_eq!(
        selected(&view, "docs/guide.md")?,
        [("c".to_owned(), "input:docs/guide.md".to_owned())]
    );
    Ok(())
}

#[test]
fn an_include_in_a_proc_macro_crate_is_unresolved() -> TestResult {
    let source = "const A: &str = include_str!(\"../../../docs/guide.md\");";
    for (kind, selects) in [(TargetKind::ProcMacro, true), (TargetKind::Lib, false)] {
        let view = repo(
            vec![(
                "Cargo.toml",
                vec![
                    package("c", "crates/c/Cargo.toml", &[], &[("c", kind, true)])?,
                    lib("app", "crates/app/Cargo.toml")?,
                ],
            )],
            &[
                ("crates/c/src/lib.rs", source),
                ("crates/app/src/lib.rs", ""),
                ("docs/guide.md", ""),
            ],
        )?;
        assert_eq!(
            selected(&view, "docs/other.md")?,
            if selects {
                vec![("c".to_owned(), "unresolved-include".to_owned())]
            } else {
                Vec::new()
            },
            "{kind:?}"
        );
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
