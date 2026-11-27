// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoView;

use super::embed::{audited, selected, view_of};
use crate::facts::TestResult;
use crate::select::path;

fn nothing() -> Vec<(String, String)> {
    Vec::new()
}

fn unresolved() -> Vec<(String, String)> {
    vec![("app".to_owned(), "unresolved-include".to_owned())]
}

fn reader(source: &str, tracked: &[&str]) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = view_of(
        &["app", "tool"],
        &[
            ("crates/app/src/lib.rs", source),
            ("crates/tool/src/lib.rs", ""),
        ],
    )?;
    for file in tracked {
        view.tracked_paths.insert(path(file));
    }
    Ok(view)
}

#[test]
fn an_include_through_a_tracked_symlink_a_submodule_or_an_untracked_file_is_unresolved()
-> TestResult {
    let source = "include_str!(\"../../../PLACE/guide.md\");";
    let mut linked = reader(
        &source.replace("PLACE", "linked"),
        &["linked", "linked/guide.md"],
    )?;
    linked.symlinks.insert(path("linked"));
    assert_eq!(selected(&linked, "docs/other.md")?, unresolved());
    let mut submodule = reader(&source.replace("PLACE", "sub"), &["sub", "sub/guide.md"])?;
    submodule.gitlinks.insert(path("sub"));
    assert_eq!(selected(&submodule, "docs/other.md")?, unresolved());
    let untracked = reader(&source.replace("PLACE", "docs"), &[])?;
    assert_eq!(selected(&untracked, "docs/other.md")?, unresolved());
    let plain = reader(&source.replace("PLACE", "docs"), &["docs/guide.md"])?;
    assert_eq!(selected(&plain, "docs/other.md")?, nothing());
    assert_eq!(
        selected(&plain, "docs/guide.md")?,
        [("app".to_owned(), "input:docs/guide.md".to_owned())]
    );
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
    ] {
        let mut view = reader(source, &["docs/guide.md"])?;
        audited(&mut view)?;
        assert_eq!(selected(&view, "docs/other.md")?, unresolved(), "{source}");
    }
    let mut plain = reader(
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs/guide.md\"));",
        &["docs/guide.md"],
    )?;
    audited(&mut plain)?;
    assert_eq!(selected(&plain, "docs/other.md")?, nothing());
    Ok(())
}
