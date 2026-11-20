// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

use crate::fixture::{TestResult, files, plz_edited};

const TOOLS_PLZ: &str = "[workspace]\npr_branch_prefix = \"tools-\"\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\n";

fn refusal(files: &ReleaseFiles) -> Option<ReleaseError> {
    ReleasePlan::read(files)
        .and_then(|plan| plan.refuse_roots_below_git_root())
        .err()
}

fn with_tools_root(roots: &str, extra: &str) -> ReleaseFiles {
    let mut files = files();
    files.release_toml = Some(format!("roots = {roots}\n"));
    files.release_plz_toml.insert(
        "tools".to_owned(),
        format!("{TOOLS_PLZ}{extra}\n[[package]]\nname = \"tools\"\n"),
    );
    files
}

#[test]
fn a_release_root_below_the_git_root_is_refused_whatever_it_publishes() {
    for extra in [
        "git_only = true\n",
        "publish = false\n",
        "publish_no_verify = true\npublish_timeout = \"5m\"\n",
    ] {
        let error = refusal(&with_tools_root("[\".\", \"tools\"]", extra));
        assert!(
            matches!(&error, Some(ReleaseError::NestedReleasePlzRoot { root }) if root == "tools"),
            "{extra:?}: {error:?}"
        );
        let text = error.map_or_default(|e| e.to_string());
        assert!(text.contains("tools"), "{text}");
    }
}

#[test]
fn a_release_root_at_the_git_root_is_planned() -> TestResult {
    for (from, to) in [
        ("publish_timeout = \"5m\"\n", "git_only = true\n"),
        ("publish_timeout = \"5m\"\n", "publish = false\n"),
        ("publish_timeout = \"5m\"\n", "publish_timeout = \"5m\"\n"),
    ] {
        assert_eq!(refusal(&plz_edited(from, to)?), None, "{to}");
    }
    Ok(())
}

#[test]
fn a_directory_below_the_git_root_that_is_not_a_release_root_is_unaffected() {
    let files = with_tools_root("[\".\"]", "git_only = true\n");
    assert!(files.release_plz_toml.contains_key("tools"));
    assert_eq!(refusal(&files), None);
}
