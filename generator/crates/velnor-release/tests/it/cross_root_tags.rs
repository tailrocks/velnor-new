// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{PLZ, TestResult, files, refusal, with_metadata};

const ROOT_A: &str = "[workspace]\npr_branch_prefix = \"a-\"\ngit_tag_name = \"{{ package }}{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";

fn two_roots(tool_tag: &str) -> velnor_release::ReleaseFiles {
    let tools = PLZ.replace("name = \"app\"", "name = \"tool\"").replace(
        "name = \"tool\"\n",
        &format!("name = \"tool\"\ngit_tag_name = \"{tool_tag}\"\n"),
    );
    let mut files = files();
    files.release_toml = Some("roots = [\"a\", \"tools\"]\n".to_owned());
    files
        .release_plz_toml
        .insert("a".to_owned(), ROOT_A.to_owned());
    files.release_plz_toml.insert("tools".to_owned(), tools);
    with_metadata(
        &mut files,
        &[
            ("a/Cargo.toml", &[("a", "a/Cargo.toml")]),
            ("tools/Cargo.toml", &[("tool", "tools/Cargo.toml")]),
        ],
    );
    files
}

#[test]
fn an_inherited_tag_of_one_root_is_compared_with_a_listed_tag_of_another() -> TestResult {
    let error = refusal(&two_roots("a{{ version }}"));
    assert!(
        matches!(
            &error,
            Some(ReleaseError::DuplicateTagPattern { .. } | ReleaseError::TagsMayCollide { .. })
        ),
        "{error:?}"
    );
    let text = error.map(|e| e.to_string()).unwrap_or_default();
    assert!(text.contains("a{{ version }}"), "{text}");
    assert_eq!(refusal(&two_roots("tool-v{{ version }}")), None);
    Ok(())
}
