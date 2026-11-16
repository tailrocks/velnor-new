// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{
    MISE_LOCK, MISE_TOML, PLZ, TestResult, edit, files, locked, plz_edited, refusal,
};

#[test]
fn release_plz_missing_from_mise_is_refused() {
    let mut no_toml = files();
    no_toml.mise_tomls = vec!["[tools]\nbun = \"1.4.4\"\n".to_owned()];
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
    aqua.mise_tomls =
        vec![MISE_TOML.replace("release-plz =", "\"aqua:release-plz/release-plz\" =")];
    aqua.mise_lock = locked("aqua:release-plz/release-plz", "0.4.3");
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
fn two_selectors_that_resolve_to_different_release_plz_versions_are_refused() {
    let mut files = files();
    files.mise_tomls = vec![
        "[tools]\nrelease-plz = \"0.4.3\"\n\"aqua:release-plz/release-plz\" = \"0.4.2\"\n"
            .to_owned(),
    ];
    files.mise_lock = format!(
        "{MISE_LOCK}\n{}",
        locked("aqua:release-plz/release-plz", "0.4.2").replace("v0.4.3", "v0.4.2")
    );
    assert_eq!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzVersionsConflict {
            first: "0.4.2".to_owned(),
            second: "0.4.3".to_owned()
        })
    );
    files.mise_tomls = vec![
        "[tools]\nrelease-plz = \"0.4.3\"\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n"
            .to_owned(),
    ];
    files.mise_lock = format!(
        "{MISE_LOCK}\n{}",
        locked("aqua:release-plz/release-plz", "0.4.3")
    );
    assert_eq!(refusal(&files), None);
}

#[test]
fn the_locked_tool_must_be_the_configured_one() {
    let mut files = files();
    files.mise_lock = locked("aqua:release-plz/release-plz", "0.4.3");
    assert_eq!(refusal(&files), Some(ReleaseError::ReleasePlzNotInMiseLock));
}

#[test]
fn the_locked_version_must_equal_the_configured_one() {
    let mut stale = files();
    stale.mise_lock = "[[tools.release-plz]]\nversion = \"0.4.2\"\n".to_owned();
    assert!(matches!(
        refusal(&stale),
        Some(ReleaseError::ReleasePlzLockMismatch { configured, locked })
            if configured == "0.4.3" && locked == ["0.4.2"]
    ));

    let mut table = files();
    table.mise_tomls = vec!["[tools.release-plz]\nversion = \"0.4.3\"\n".to_owned()];
    assert_eq!(refusal(&table), None);
    table.mise_lock = "[[tools.release-plz]]\nversion = \"0.4.2\"\n".to_owned();
    assert!(matches!(
        refusal(&table),
        Some(ReleaseError::ReleasePlzLockMismatch { .. })
    ));

    let mut mixed = files();
    mixed.mise_lock = "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[[tools.release-plz]]\nversion = \"0.4.2\"\n".to_owned();
    assert!(matches!(
        refusal(&mixed),
        Some(ReleaseError::ReleasePlzLockMismatch { configured, locked })
            if configured == "0.4.3" && locked == ["0.4.3", "0.4.2"]
    ));

    let mut longer = files();
    longer.mise_lock = "[[tools.release-plz]]\nversion = \"0.4.30\"\n".to_owned();
    assert!(matches!(
        refusal(&longer),
        Some(ReleaseError::ReleasePlzLockMismatch { configured, locked })
            if configured == "0.4.3" && locked == ["0.4.30"]
    ));

    let mut empty = files();
    empty.mise_lock = "[tools]\nrelease-plz = []\n".to_owned();
    assert!(matches!(
        refusal(&empty),
        Some(ReleaseError::ReleasePlzLockMismatch { .. })
    ));

    let mut unversioned = files();
    unversioned.mise_tomls = vec!["[tools.release-plz]\n".to_owned()];
    unversioned.mise_lock = "[[tools.release-plz]]\n".to_owned();
    assert!(matches!(
        refusal(&unversioned),
        Some(ReleaseError::ReleasePlzVersionNotExact { .. })
    ));

    let mut blank = files();
    blank.mise_tomls = vec!["[tools]\nrelease-plz = \"\"\n".to_owned()];
    blank.mise_lock = "[[tools.release-plz]]\nversion = \"\"\n".to_owned();
    assert!(matches!(
        refusal(&blank),
        Some(ReleaseError::ReleasePlzVersionNotExact { .. })
    ));
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
fn a_package_released_from_two_roots_is_refused() -> TestResult {
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
        matches!(refusal(&files), Some(ReleaseError::DuplicatePackageName { package, .. }) if package == "app")
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
    for name in ["../x", "\u{e9}foo", "a b", "1app", ""] {
        let bad = plz_edited("name = \"app\"", &format!("name = {name:?}"))?;
        assert!(
            matches!(refusal(&bad), Some(ReleaseError::InvalidPackageName { .. })),
            "{name:?}"
        );
    }
    let good = plz_edited("name = \"app\"", "name = \"a-b_c\"")?;
    assert_eq!(refusal(&good), None);
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

#[test]
fn a_bin_name_that_is_not_a_plain_binary_name_is_refused() {
    for bin in ["rg; echo", "a.b", "", "-x", "a b", "$(x)"] {
        let mut files = files();
        files.release_toml = Some(format!("[[binaries]]\npackage = \"app\"\nbin = {bin:?}\n"));
        assert!(
            matches!(refusal(&files), Some(ReleaseError::InvalidBinName { .. })),
            "{bin:?}"
        );
    }
}

#[test]
fn two_binaries_entries_for_one_package_are_refused() {
    let mut files = files();
    files.release_toml = Some(
        "[[binaries]]\npackage = \"app\"\n[[binaries]]\npackage = \"app\"\nbin = \"rg\"\n"
            .to_owned(),
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::DuplicateBinary { package }) if package == "app"),
        "{error:?}"
    );
    assert!(error.is_some_and(|error| error.to_string().contains("app")));
}

#[test]
fn an_after_list_that_misses_a_workflow_that_runs_on_push_is_refused() {
    let pushed = |names: &[&str]| Some(names.iter().map(|name| (*name).to_owned()).collect());
    let mut files = files();
    files.release_toml = Some("after = [\"Rust CI\"]\n".to_owned());
    files.push_workflows = pushed(&["Rust CI", "Lint"]);
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::AfterMissingWorkflow { name }) if name == "Lint"
    ));
    files.push_workflows = pushed(&["Rust CI"]);
    assert_eq!(refusal(&files), None);
    files.release_toml = Some("after = [\"Rust CI\", \"Lint\"]\n".to_owned());
    files.push_workflows = pushed(&["Rust CI", "Lint"]);
    assert_eq!(refusal(&files), None);
}
