// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{TestResult, files, plz_edited, refusal};

const NESTED_PLZ: &str = "[workspace]\npr_branch_prefix = \"tools-\"\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";

fn with_nested_root(tools: &str) -> velnor_release::ReleaseFiles {
    let mut files = files();
    files.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    files
        .release_plz_toml
        .insert("tools".to_owned(), tools.to_owned());
    files
}

#[test]
fn a_git_only_root_below_the_git_root_is_refused() {
    for tools in [
        format!("{NESTED_PLZ}git_only = true\n\n[[package]]\nname = \"tools\"\n"),
        format!("{NESTED_PLZ}\n[[package]]\nname = \"tools\"\ngit_only = true\n"),
    ] {
        let error = refusal(&with_nested_root(&tools));
        assert!(
            matches!(&error, Some(ReleaseError::NestedGitOnlyRoot { root }) if root == "tools"),
            "{error:?}"
        );
        let text = error.map(|e| e.to_string()).unwrap_or_default();
        assert!(text.contains("tools"), "{text}");
    }
}

#[test]
fn a_git_only_root_at_the_git_root_is_planned() -> TestResult {
    assert_eq!(
        refusal(&plz_edited(
            "publish_timeout = \"5m\"\n",
            "git_only = true\n"
        )?),
        None
    );
    Ok(())
}

#[test]
fn a_root_below_the_git_root_without_git_only_is_planned() {
    let tools = format!("{NESTED_PLZ}\n[[package]]\nname = \"tools\"\n");
    assert_eq!(refusal(&with_nested_root(&tools)), None);
}
