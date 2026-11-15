// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{MISE_TOML, PLZ, TestResult, edit, files, plz_edited, refusal};

#[test]
fn release_plz_missing_from_mise_is_refused() {
    let mut no_toml = files();
    no_toml.mise_toml = "[tools]\nbun = \"1.4.4\"\n".to_owned();
    assert_eq!(
        refusal(&no_toml),
        Some(ReleaseError::ReleasePlzNotInMiseToml)
    );

    let mut no_lock = files();
    no_lock.mise_lock = "[[tools.bun]]\nversion = \"1.4.4\"\n".to_owned();
    assert_eq!(
        refusal(&no_lock),
        Some(ReleaseError::ReleasePlzNotInMiseLock)
    );

    let mut aqua = files();
    aqua.mise_toml = MISE_TOML.replace("release-plz =", "\"aqua:release-plz/release-plz\" =");
    aqua.mise_lock = "[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(refusal(&aqua), None);
}

#[test]
fn a_binary_that_release_plz_does_not_release_is_refused() -> TestResult {
    let binaries = "[[binaries]]\npackage = \"tool\"\n";
    let mut absent = files();
    absent.release_toml = Some(binaries.to_owned());
    assert!(
        matches!(refusal(&absent), Some(ReleaseError::BinaryNotReleased { package }) if package == "tool")
    );

    let mut unreleased = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"tool\"\nrelease = false",
    )?;
    unreleased.release_toml = Some(binaries.to_owned());
    assert!(matches!(
        refusal(&unreleased),
        Some(ReleaseError::BinaryNotReleased { .. })
    ));

    let mut listed = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"tool\"",
    )?;
    listed.release_toml = Some(binaries.to_owned());
    assert_eq!(refusal(&listed), None);
    Ok(())
}

#[test]
fn the_locked_tool_must_be_the_configured_one() {
    let mut files = files();
    files.mise_lock =
        "[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(refusal(&files), Some(ReleaseError::ReleasePlzNotInMiseLock));
}

#[test]
fn release_config_with_an_empty_after_or_unknown_platform_is_refused() {
    let mut files = files();
    files.release_toml = Some("after = []\n".to_owned());
    assert_eq!(refusal(&files), Some(ReleaseError::EmptyAfter));
    files.release_toml =
        Some("[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-amd64\"]\n".to_owned());
    assert!(
        matches!(refusal(&files), Some(ReleaseError::UnknownPlatform { platform, .. }) if platform == "linux-amd64")
    );
}

#[test]
fn a_binary_released_from_two_roots_is_refused() -> TestResult {
    let mut files = files();
    files.release_toml =
        Some("roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\n".to_owned());
    let tools = edit(
        PLZ,
        "[workspace]\n",
        "[workspace]\npr_branch_prefix = \"tools-\"\n",
    )?;
    let tools = edit(&tools, "{{ package }}-v", "tools-{{ package }}-v")?;
    files.release_plz_toml.insert("tools".to_owned(), tools);
    assert!(
        matches!(refusal(&files), Some(ReleaseError::AmbiguousBinary { package, .. }) if package == "app")
    );
    Ok(())
}

#[test]
fn empty_roots_and_blank_workflow_names_are_refused() {
    let mut files = files();
    files.release_toml = Some("roots = []\n".to_owned());
    assert_eq!(refusal(&files), Some(ReleaseError::EmptyRoots));
    files.release_toml = Some("after = [\"\"]\n".to_owned());
    assert_eq!(refusal(&files), Some(ReleaseError::EmptyAfter));
}

#[test]
fn unusable_roots_and_empty_platform_lists_are_refused() {
    for root in ["/etc", "../outside", "a/../b"] {
        let mut files = files();
        files.release_toml = Some(format!("roots = [{root:?}]\n"));
        assert!(
            matches!(refusal(&files), Some(ReleaseError::UnsafeRoot { .. })),
            "{root}"
        );
    }
    let mut files = files();
    files.release_toml = Some("[[binaries]]\npackage = \"app\"\nplatforms = []\n".to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::EmptyPlatforms { .. })
    ));
}

#[test]
fn duplicate_package_entries_and_bad_package_names_are_refused() -> TestResult {
    let duplicate = plz_edited(
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"app\"",
    )?;
    assert!(
        matches!(refusal(&duplicate), Some(ReleaseError::DuplicatePackage { package, .. }) if package == "app")
    );
    for name in ["../x", "\u{e9}foo", "a b", ""] {
        let bad = plz_edited("name = \"app\"", &format!("name = {name:?}"))?;
        assert!(
            matches!(refusal(&bad), Some(ReleaseError::InvalidPackageName { .. })),
            "{name:?}"
        );
    }
    Ok(())
}

#[test]
fn a_root_whose_workspace_releases_nothing_is_refused() -> TestResult {
    let text = edit(
        &PLZ.replace("[[package]]\nname = \"app\"\n", ""),
        "[workspace]\n",
        "[workspace]\nrelease = false\n",
    )?;
    let mut files = files();
    files.release_plz_toml.insert(".".to_owned(), text);
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::RootReleasesNothing { .. })
    ));
    Ok(())
}
