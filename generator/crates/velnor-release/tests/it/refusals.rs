// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleasePlan};

use crate::fixture::{MISE_TOML, PLZ, TestResult, edit, files, plz_edited, refusal};

type Case = (&'static str, &'static str, &'static str);

fn plz_refusal(from: &str, to: &str) -> Result<Option<ReleaseError>, String> {
    Ok(refusal(&plz_edited(from, to)?))
}

#[test]
fn a_package_without_an_explicit_tag_name_is_refused() -> TestResult {
    let error = plz_refusal("git_tag_name = \"{{ package }}-v{{ version }}\"\n", "")?;
    assert!(
        matches!(&error, Some(ReleaseError::MissingGitTagName { package: Some(p), .. }) if p == "app"),
        "{error:?}"
    );
    let rescued = edit(
        PLZ,
        "name = \"app\"",
        "name = \"app\"\ngit_tag_name = \"app-v{{ version }}\"",
    )?;
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        edit(
            &rescued,
            "git_tag_name = \"{{ package }}-v{{ version }}\"\n",
            "",
        )?,
    );
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn a_tag_template_with_anything_but_package_and_version_is_refused() -> TestResult {
    let templates = [
        "{{ name }}-v{{ version }}",
        "{{ package }}-v{{ version",
        "v{{ version | upper }}",
        "v{{ version }}}",
    ];
    for template in templates {
        let to = format!("git_tag_name = \"{template}\"");
        let error = plz_refusal("git_tag_name = \"{{ package }}-v{{ version }}\"", &to)?;
        assert!(
            matches!(error, Some(ReleaseError::InvalidTagTemplate { .. })),
            "{template}: {error:?}"
        );
    }
    let spaced = plz_refusal("{{ package }}-v{{ version }}", "{{package}}-v{{version}}")?;
    assert_eq!(spaced, None);
    Ok(())
}

#[test]
fn a_release_draft_other_than_true_is_refused() -> TestResult {
    for (from, to) in [
        ("git_release_draft = true\n", "git_release_draft = false\n"),
        ("git_release_draft = true\n", ""),
    ] {
        let error = plz_refusal(from, to)?;
        assert!(
            matches!(error, Some(ReleaseError::GitReleaseDraftNotTrue { .. })),
            "{error:?}"
        );
    }
    Ok(())
}

#[test]
fn release_always_other_than_false_is_refused() -> TestResult {
    for (from, to) in [
        ("release_always = false\n", "release_always = true\n"),
        ("release_always = false\n", ""),
    ] {
        let error = plz_refusal(from, to)?;
        assert!(
            matches!(error, Some(ReleaseError::ReleaseAlwaysNotFalse { .. })),
            "{error:?}"
        );
    }
    Ok(())
}

#[test]
fn semver_check_other_than_false_is_refused() -> TestResult {
    for (from, to) in [
        ("semver_check = false\n", "semver_check = true\n"),
        ("semver_check = false\n", ""),
    ] {
        let error = plz_refusal(from, to)?;
        assert!(
            matches!(error, Some(ReleaseError::SemverCheckNotFalse { .. })),
            "{error:?}"
        );
    }
    Ok(())
}

#[test]
fn crates_io_publishing_needs_publish_no_verify() -> TestResult {
    for to in ["", "publish_no_verify = false\n"] {
        let error = plz_refusal("publish_no_verify = true\n", to)?;
        assert!(
            matches!(error, Some(ReleaseError::PublishWithoutNoVerify { .. })),
            "{error:?}"
        );
    }
    let unpublished = plz_refusal("publish_no_verify = true\n", "publish = false\n")?;
    assert_eq!(unpublished, None);
    Ok(())
}

#[test]
fn two_packages_with_the_same_tag_pattern_are_refused() -> TestResult {
    let cases: [Case; 2] = [
        (
            "{{ package }}-v{{ version }}",
            "v{{ version }}",
            "name = \"app\"\n\n[[package]]\nname = \"lib\"",
        ),
        (
            "name = \"app\"",
            "name = \"app\"\n\n[[package]]\nname = \"lib\"\ngit_tag_name = \"app-v{{ version }}\"",
            "",
        ),
    ];
    for (from, to, extra) in cases {
        let mut text = edit(PLZ, from, to)?;
        if !extra.is_empty() {
            text = edit(&text, "name = \"app\"", extra)?;
        }
        let mut files = files();
        files.release_plz_toml.insert(".".to_owned(), text);
        let error = refusal(&files);
        assert!(
            matches!(error, Some(ReleaseError::DuplicateTagPattern { .. })),
            "{error:?}"
        );
    }
    let mut distinct = files();
    distinct.release_plz_toml.insert(
        ".".to_owned(),
        edit(
            PLZ,
            "name = \"app\"",
            "name = \"app\"\n\n[[package]]\nname = \"lib\"",
        )?,
    );
    assert_eq!(refusal(&distinct), None);
    Ok(())
}

#[test]
fn two_roots_with_the_same_pr_branch_prefix_are_refused() -> TestResult {
    let mut files = files();
    files.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    files.release_plz_toml.insert(
        "tools".to_owned(),
        edit(PLZ, "name = \"app\"", "name = \"cli\"")?,
    );
    let error = refusal(&files);
    assert!(
        matches!(error, Some(ReleaseError::DuplicatePrBranchPrefix { .. })),
        "{error:?}"
    );

    let with_prefix = edit(
        PLZ,
        "[workspace]\n",
        "[workspace]\npr_branch_prefix = \"tools-\"\n",
    )?;
    let tools = edit(&with_prefix, "name = \"app\"", "name = \"cli\"")?;
    files.release_plz_toml.insert("tools".to_owned(), tools);
    assert_eq!(ReleasePlan::read(&files)?.roots.len(), 2);
    Ok(())
}

#[test]
fn a_root_without_its_release_plz_toml_is_refused() {
    let mut files = files();
    files.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    assert!(
        matches!(refusal(&files), Some(ReleaseError::MissingReleasePlzToml { root }) if root == "tools")
    );
}

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
fn a_root_without_package_entries_is_checked_at_the_workspace_level() -> TestResult {
    let mut empty = files();
    empty.release_plz_toml.insert(
        ".".to_owned(),
        PLZ.replace("[[package]]\nname = \"app\"\n", ""),
    );
    assert_eq!(refusal(&empty), None);

    let no_tag = edit(
        &empty.release_plz_toml["."],
        "git_tag_name = \"{{ package }}-v{{ version }}\"\n",
        "",
    )?;
    empty.release_plz_toml.insert(".".to_owned(), no_tag);
    assert!(matches!(
        refusal(&empty),
        Some(ReleaseError::MissingGitTagName { package: None, .. })
    ));

    let mut draft = files();
    draft.release_plz_toml.insert(
        ".".to_owned(),
        edit(
            &PLZ.replace("[[package]]\nname = \"app\"\n", ""),
            "git_release_draft = true",
            "git_release_draft = false",
        )?,
    );
    assert!(matches!(
        refusal(&draft),
        Some(ReleaseError::GitReleaseDraftNotTrue { package: None, .. })
    ));
    Ok(())
}

#[test]
fn errors_read_as_messages_naming_the_file_and_package() {
    let mut files = files();
    files.release_toml = Some("[[binaries]]\npackage = \"tool\"\n".to_owned());
    let message = refusal(&files)
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("tool") && message.contains(".velnor/release.toml"),
        "{message}"
    );
    let message = plz_refusal("semver_check = false", "semver_check = true")
        .ok()
        .flatten()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("./release-plz.toml")
            && message.contains("semver_check")
            && message.contains("app"),
        "{message}"
    );
}
