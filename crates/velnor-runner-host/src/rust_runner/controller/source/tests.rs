// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::root_workflow_path;

const REPOSITORY: &str = "owner/repo";

#[test]
fn an_at_sign_in_the_file_name_is_not_the_ref_separator() {
    assert_eq!(
        root_workflow_path(
            "owner/repo/.github/workflows/release@nightly.yml@refs/heads/main",
            REPOSITORY
        )
        .as_deref(),
        Ok(".github/workflows/release@nightly.yml")
    );
    assert_eq!(
        root_workflow_path(".github/workflows/release@nightly.yml", REPOSITORY).as_deref(),
        Ok(".github/workflows/release@nightly.yml")
    );
}

#[test]
fn the_ref_is_dropped_whatever_it_contains() {
    for path in [
        ".github/workflows/ci.yml@refs/heads/main",
        "./.github/workflows/ci.yml@refs/heads/feature@2",
        "owner/repo/.github/workflows/ci.yml@refs/tags/v1.yml@x",
        ".github/workflows/ci.yml",
    ] {
        assert_eq!(
            root_workflow_path(path, REPOSITORY).as_deref(),
            Ok(".github/workflows/ci.yml"),
            "{path}"
        );
    }
    assert_eq!(
        root_workflow_path(".github/workflows/ci.yaml@refs/heads/main", REPOSITORY).as_deref(),
        Ok(".github/workflows/ci.yaml")
    );
}

#[test]
fn the_file_name_ends_at_the_last_at_sign_before_the_first_slash_of_the_ref() {
    for (path, expected) in [
        (
            "owner/repo/.github/workflows/a.yml@refs/heads/x.yaml@y",
            ".github/workflows/a.yml",
        ),
        (
            ".github/workflows/a.yaml@b.yml@refs/heads/x.yml@y",
            ".github/workflows/a.yaml@b.yml",
        ),
        (
            ".github/workflows/a@b.yml@refs/tags/v1.yml@x",
            ".github/workflows/a@b.yml",
        ),
    ] {
        assert_eq!(
            root_workflow_path(path, REPOSITORY).as_deref(),
            Ok(expected),
            "{path}"
        );
    }
}
