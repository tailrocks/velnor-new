// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

fn targets(items: &[(&str, &str)]) -> BTreeSet<(String, String, String)> {
    items
        .iter()
        .map(|(package, bin)| {
            (
                "Cargo.toml".to_owned(),
                (*package).to_owned(),
                (*bin).to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_bin_name_that_another_selected_package_also_defines_is_refused() {
    let mut files = files();
    files.release_toml = Some(
        "[[binaries]]\npackage = \"app\"\nbins = [{ package = \"lib\", bin = \"rg\" }]\n"
            .to_owned(),
    );
    files.bin_targets = Some(targets(&[("app", "app"), ("lib", "rg"), ("app", "rg")]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::InvalidBundle { entry, .. }) if entry == "rg"),
        "{error:?}"
    );
    files.bin_targets = Some(targets(&[("app", "app"), ("lib", "rg")]));
    assert_eq!(refusal(&files), None);
}

#[test]
fn an_include_that_is_a_directory_is_refused() {
    let mut files = files();
    files.release_toml =
        Some("[[binaries]]\npackage = \"app\"\ninclude = [\"docs\", \"LICENSE\"]\n".to_owned());
    files.tracked_paths = Some(
        ["LICENSE", "docs/guide.md"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::InvalidBundle { entry, .. }) if entry == "docs"),
        "{error:?}"
    );
    files.release_toml = Some(
        "[[binaries]]\npackage = \"app\"\ninclude = [\"docs/guide.md\", \"LICENSE\"]\n".to_owned(),
    );
    assert_eq!(refusal(&files), None);
}

#[test]
fn an_asset_name_too_long_for_a_runner_file_name_is_refused() {
    let mut files = files();
    let long = "a".repeat(240);
    files.release_toml = Some(format!(
        "[[binaries]]\npackage = \"app\"\nasset_name = \"{{{{ bin }}}}-{{{{ platform }}}}-{long}\"\n"
    ));
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::InvalidAssetName { .. })
    ));
    let fits = "a".repeat(150);
    files.release_toml = Some(format!(
        "[[binaries]]\npackage = \"app\"\nasset_name = \"{{{{ bin }}}}-{{{{ platform }}}}-{fits}\"\n"
    ));
    assert_eq!(refusal(&files), None);
}
