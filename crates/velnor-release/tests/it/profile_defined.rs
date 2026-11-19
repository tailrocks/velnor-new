// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

#[test]
fn a_profile_missing_from_the_root_manifest_is_refused_at_generation() {
    let binaries = "[[binaries]]\npackage = \"app\"\nprofile = \"release-tools\"\n";
    let mut files = files();
    files.release_toml = Some(binaries.to_owned());
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = []\n".to_owned(),
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::UndefinedProfile(package, profile, _))
            if package == "app" && profile == "release-tools"
    ));

    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[profile.release-tools]\ninherits = \"release\"\n".to_owned(),
    );
    assert_eq!(refusal(&files), None);

    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), "[workspace]\n".to_owned());
    files.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[profile.release-tools]\ninherits = \"release\"\n".to_owned(),
    );
    assert_eq!(refusal(&files), None);

    files.cargo_configs.clear();
    files.release_toml =
        Some("[[binaries]]\npackage = \"app\"\nprofile = \"release\"\n".to_owned());
    assert_eq!(refusal(&files), None);

    files.release_toml = Some(binaries.to_owned());
    files.cargo_manifest_texts.clear();
    assert_eq!(refusal(&files), None);
}
