// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

fn expression_refusal(release_toml: &str) -> Option<ReleaseError> {
    let mut files = files();
    files.release_toml = Some(release_toml.to_owned());
    refusal(&files)
}

#[test]
fn a_root_containing_an_expression_is_refused_naming_the_file_and_key() {
    let error = expression_refusal("roots = [\"${{ github.token }}\"]\n");
    assert!(
        matches!(&error, Some(ReleaseError::ExpressionInValue { key, .. }) if key == "roots"),
        "{error:?}"
    );
    assert!(
        error.is_some_and(|error| error.to_string().starts_with(".velnor/release.toml: roots"))
    );
}

#[test]
fn a_bin_name_containing_an_expression_is_refused_naming_the_file_and_key() {
    let error =
        expression_refusal("[[binaries]]\npackage = \"app\"\nbin = \"${{ github.token }}\"\n");
    assert!(
        matches!(&error, Some(ReleaseError::ExpressionInValue { key, .. }) if key == "binaries.bin"),
        "{error:?}"
    );
    assert!(error.is_some_and(|error| {
        error
            .to_string()
            .starts_with(".velnor/release.toml: binaries.bin")
    }));
}

#[test]
fn a_workflow_name_containing_an_expression_is_refused_naming_the_file_and_key() {
    let error = expression_refusal("after = [\"Rust CI ${{ github.token }}\"]\n");
    assert!(
        matches!(&error, Some(ReleaseError::ExpressionInValue { key, .. }) if key == "after"),
        "{error:?}"
    );
    assert!(
        error.is_some_and(|error| error.to_string().starts_with(".velnor/release.toml: after"))
    );
}

#[test]
fn a_package_name_containing_an_expression_is_refused_naming_the_file_and_key() {
    let error = expression_refusal("[[binaries]]\npackage = \"${{ github.token }}\"\n");
    assert!(
        matches!(&error, Some(ReleaseError::ExpressionInValue { key, .. }) if key == "binaries.package"),
        "{error:?}"
    );
}
