// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{InputKey, InputOwner, RepoView, RustSelection};
use velnor_rust::detect;

use crate::facts::{TestResult, lib, repo};
use crate::pipeline::HELPER_LOCK;
use crate::select::{name, path};

pub(super) fn view_of(
    members: &[&str],
    files: &[(&str, &str)],
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let crates = members
        .iter()
        .map(|member| lib(member, &format!("crates/{member}/Cargo.toml")))
        .collect::<Result<Vec<_>, _>>()?;
    repo(vec![("Cargo.toml", crates)], files)
}

pub(super) fn selections(
    members: &[&str],
    files: &[(&str, &str)],
    changed_file: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    selected(&view_of(members, files)?, changed_file)
}

pub(super) fn audited(view: &mut RepoView) -> TestResult {
    pinned(view, "0.1.117")
}

pub(super) fn pinned(view: &mut RepoView, version: &str) -> TestResult {
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.81", version, 1);
    view.files
        .insert(path(".velnor/rust-ci.lock"), lock.into_bytes());
    Ok(())
}

pub(super) fn selected(
    view: &RepoView,
    changed_file: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let facts = detect(view)?;
    let changes = velnor_model::ChangeSet {
        paths: BTreeSet::from([path(changed_file)]),
        base_files: BTreeMap::new(),
        head_files: BTreeMap::new(),
    };
    Ok(
        match velnor_rust::select(&facts, &changes).into_values().next() {
            Some(RustSelection::Some(map)) => map
                .into_iter()
                .map(|(name, reason)| (name.to_string(), reason.to_string()))
                .collect(),
            Some(RustSelection::All(_)) => vec![("*".to_owned(), String::new())],
            _ => Vec::new(),
        },
    )
}

pub(super) fn embedding(
    files: &[(&str, &str)],
    check: &[(&str, &[&str])],
    evidence: Evidence,
) -> TestResult {
    let mut view = view_of(&["app", "tool", "reader", "left"], files)?;
    if evidence == Evidence::Audited {
        audited(&mut view)?;
    }
    for (file, _) in check {
        view.tracked_paths.insert(path(file));
    }
    for (file, expected) in check {
        let names: Vec<String> = selected(&view, file)?
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(&names, expected, "{file}");
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Evidence {
    Audited,
    SourceOnly,
}

pub(super) fn unresolved(source: &str, evidence: Evidence) -> TestResult {
    let mut view = view_of(
        &["app", "c", "tool", "left"],
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
            ("crates/c/src/x.md", ""),
            ("x.md", ""),
        ],
    )?;
    if evidence == Evidence::Audited {
        audited(&mut view)?;
    }
    assert_eq!(
        selected(&view, "docs/other.md")?,
        [("c".to_owned(), "unresolved-include".to_owned())],
        "{source}"
    );
    Ok(())
}

#[test]
fn a_file_a_crate_embeds_selects_it_without_an_inputs_lock_line() -> TestResult {
    embedding(
        &[
            (
                "crates/app/src/lib.rs",
                "const A: &str = include_str!(\"../../../docs/guide.md\");\nconst H: &[u8] = include_bytes!(\"../../../shared/header.h\");",
            ),
            (
                "crates/tool/src/lib.rs",
                "const H: &str = include_str!(\"../../../shared/header.h\");",
            ),
            (
                "crates/reader/src/lib.rs",
                "const B: &str = include_str!(\"../../left/src/banner.txt\");",
            ),
            ("crates/left/src/lib.rs", ""),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("shared/header.h", &["app", "tool"]),
            ("crates/left/src/banner.txt", &["left", "reader"]),
            ("docs/other.md", &[]),
        ],
        Evidence::SourceOnly,
    )
}

#[test]
fn a_file_an_included_file_embeds_selects_the_crate() -> TestResult {
    embedding(
        &[
            ("crates/app/src/lib.rs", "include!(\"gen.rs\");"),
            (
                "crates/app/src/gen.rs",
                "const A: &str = include_str!(\"../../../docs/guide.md\");\ninclude!(\"deep.rs\");",
            ),
            (
                "crates/app/src/deep.rs",
                "const B: &str = include_str!(\"../../../docs/deep.md\");",
            ),
            ("crates/tool/src/lib.rs", ""),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("docs/deep.md", &["app"]),
            ("docs/other.md", &[]),
        ],
        Evidence::SourceOnly,
    )
}

#[test]
fn an_included_file_the_view_cannot_read_fails_closed() -> TestResult {
    let mut view = view_of(
        &["app", "tool"],
        &[
            ("crates/app/src/lib.rs", "include!(\"body.inc\");"),
            ("crates/tool/src/lib.rs", ""),
        ],
    )?;
    view.tracked_paths.insert(path("crates/app/src/body.inc"));
    assert_eq!(
        selected(&view, "docs/other.md")?,
        [("app".to_owned(), "unresolved-include".to_owned())]
    );
    Ok(())
}

#[test]
fn a_comment_between_the_macro_and_its_bang_does_not_hide_it() -> TestResult {
    embedding(
        &[
            (
                "crates/app/src/lib.rs",
                "const A: &str = include_str/* generated */!(\"../../../docs/guide.md\");\nconst B: &str = include_str // x\n!(\"../../../docs/b.md\");",
            ),
            (
                "crates/tool/src/lib.rs",
                "// include_str!(\"../../../docs/commented.md\")\nconst S: &str = \"include_str!(\\\"../../../docs/quoted.md\\\")\";",
            ),
        ],
        &[
            ("docs/guide.md", &["app"]),
            ("docs/b.md", &["app"]),
            ("docs/commented.md", &[]),
            ("docs/quoted.md", &[]),
        ],
        Evidence::SourceOnly,
    )
}

#[test]
fn a_nested_package_is_not_charged_with_its_ancestors_scan() -> TestResult {
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("top", "Cargo.toml")?,
                lib("inner", "crates/inner/Cargo.toml")?,
            ],
        )],
        &[
            (
                "src/lib.rs",
                "const A: &str = include_str!(\"../docs/top.md\");",
            ),
            (
                "crates/inner/src/lib.rs",
                "const B: &str = include_str!(\"../../../docs/inner.md\");",
            ),
            ("docs/top.md", ""),
            ("docs/inner.md", ""),
        ],
    )?;
    let facts = detect(&view)?;
    let owners = |file: &str| -> Option<Vec<String>> {
        facts
            .compile_inputs
            .get(&InputKey::Path(path(file)))
            .map(|owners| owners.iter().map(|owner| owner.name.to_string()).collect())
    };
    assert_eq!(owners("docs/top.md"), Some(vec!["top".to_owned()]));
    assert_eq!(owners("docs/inner.md"), Some(vec!["inner".to_owned()]));
    assert!(
        !facts.compile_inputs.contains_key(&InputKey::Unresolved),
        "{:?}",
        facts.compile_inputs
    );
    Ok(())
}

#[test]
fn a_target_source_outside_the_package_directory_attributes_its_includes() -> TestResult {
    let mut app = lib("app", "crates/app/Cargo.toml")?;
    for target in &mut app.targets {
        target.source = Some(path("shared/app_lib.rs"));
    }
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![app, lib("tool", "crates/tool/Cargo.toml")?],
        )],
        &[
            (
                "shared/app_lib.rs",
                "const A: &str = include_str!(\"../docs/guide.md\");\nmod more;",
            ),
            (
                "shared/more.rs",
                "const B: &str = include_str!(\"../docs/more.md\");",
            ),
            ("crates/tool/src/lib.rs", ""),
            ("docs/guide.md", ""),
            ("docs/more.md", ""),
        ],
    )?;
    let facts = detect(&view)?;
    for file in ["docs/guide.md", "docs/more.md"] {
        let names: Vec<String> = facts
            .compile_inputs
            .get(&InputKey::Path(path(file)))
            .into_iter()
            .flatten()
            .map(|owner| owner.name.to_string())
            .collect();
        assert_eq!(names, ["app"], "{file}");
    }
    Ok(())
}

#[test]
fn a_symlinked_rust_source_fails_closed_instead_of_being_skipped() -> TestResult {
    let mut view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("app", "crates/app/Cargo.toml")?,
                lib("tool", "crates/tool/Cargo.toml")?,
            ],
        )],
        &[("crates/tool/src/lib.rs", "")],
    )?;
    let linked = path("crates/app/src/lib.rs");
    view.tracked_paths.insert(linked.clone());
    view.symlinks.insert(linked);
    let facts = detect(&view)?;
    let app = InputOwner::at(name("app"), path("crates/app/Cargo.toml"));
    assert_eq!(
        facts.compile_inputs.get(&InputKey::Unresolved),
        Some(&BTreeSet::from([app]))
    );
    Ok(())
}
