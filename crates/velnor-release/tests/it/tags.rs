// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{PLZ, TestResult, edit, files, refusal, with_metadata};

fn two_tags(first: &str, second: &str) -> Result<Option<ReleaseError>, String> {
    let text = edit(
        PLZ,
        "name = \"app\"",
        &format!(
            "name = \"app\"\ngit_tag_name = \"{first}\"\n\n[[package]]\nname = \"tool\"\ngit_tag_name = \"{second}\""
        ),
    )?;
    let mut files = files();
    files.release_plz_toml.insert(".".to_owned(), text);
    Ok(refusal(&files))
}

#[test]
fn nested_tag_prefixes_collide_only_when_the_gap_can_start_a_version() -> TestResult {
    for (first, second, collision) in [
        ("x{{ version }}", "xa{{ version }}", false),
        ("x{{ version }}", "x1{{ version }}", true),
        ("{{ version }}", "1{{ version }}", true),
        ("{{ version }}", "a{{ version }}", false),
        ("x{{ version }}", "x{{ version }}", true),
        ("x{{ version }}", "y{{ version }}", false),
    ] {
        for (left, right) in [(first, second), (second, first)] {
            let error = two_tags(left, right)?;
            if collision {
                assert!(
                    matches!(
                        error,
                        Some(
                            ReleaseError::TagsMayCollide { .. }
                                | ReleaseError::DuplicateTagPattern { .. }
                        )
                    ),
                    "{left} / {right}: {error:?}"
                );
            } else {
                assert_eq!(error, None, "{left} / {right}");
            }
        }
    }
    Ok(())
}

#[test]
fn nested_tag_suffixes_collide_only_when_the_gap_can_belong_to_a_version() -> TestResult {
    for (first, second, collision) in [
        ("v{{ version }}", "v{{ version }}_z", false),
        ("v{{ version }}", "v{{ version }}-z", true),
        ("v{{ version }}-foo", "v{{ version }}-bar", false),
        ("{{ version }}_z", "{{ version }}_z", true),
    ] {
        for (left, right) in [(first, second), (second, first)] {
            let error = two_tags(left, right)?;
            if collision {
                assert!(
                    matches!(
                        error,
                        Some(
                            ReleaseError::TagsMayCollide { .. }
                                | ReleaseError::DuplicateTagPattern { .. }
                        )
                    ),
                    "{left} / {right}: {error:?}"
                );
            } else {
                assert_eq!(error, None, "{left} / {right}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_template_with_two_version_placeholders_is_refused() -> TestResult {
    let error = two_tags("x{{ version }}_foo{{ version }}", "x1.2.3_foo{{ version }}")?;
    assert!(
        matches!(error, Some(ReleaseError::InvalidTagTemplate { .. })),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn different_fixed_suffixes_after_the_version_are_not_a_collision() -> TestResult {
    assert_eq!(two_tags("v{{ version }}-foo", "v{{ version }}-bar")?, None);
    assert_eq!(two_tags("a{{ version }}", "b{{ version }}")?, None);
    for (first, second) in [
        ("v{{ version }}", "v{{ version }}-foo"),
        ("v{{ version }}-foo", "v1{{ version }}-foo"),
    ] {
        let error = two_tags(first, second)?;
        assert!(
            matches!(
                error,
                Some(
                    ReleaseError::TagsMayCollide { .. } | ReleaseError::DuplicateTagPattern { .. }
                )
            ),
            "{first} {second}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn a_version_that_would_end_the_tag_in_lock_is_refused_before_publication() {
    let manifest = |version: &str| format!("[package]\nname = \"app\"\nversion = \"{version}\"\n");
    let mut files = files();
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("1.2.3-alpha.lock"));
    let error = refusal(&files);
    assert!(
        matches!(error, Some(ReleaseError::VersionEndsInLock(..))),
        "{error:?}"
    );
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("1.2.3-alpha"));
    assert_eq!(refusal(&files), None);
    let suffixed = PLZ.replace(
        "{{ package }}-v{{ version }}",
        "{{ package }}-v{{ version }}-x",
    );
    files.release_plz_toml.insert(".".to_owned(), suffixed);
    files
        .cargo_manifest_texts
        .insert("Cargo.toml".to_owned(), manifest("1.2.3-alpha.lock"));
    assert_eq!(refusal(&files), None);
}

#[test]
fn crates_io_names_are_checked_for_packages_that_publish() -> TestResult {
    let long = "a".repeat(65);
    for name in ["_x", long.as_str()] {
        let error = refusal(&crate::fixture::plz_edited(
            "name = \"app\"",
            &format!("name = \"{name}\""),
        )?);
        assert!(
            matches!(&error, Some(ReleaseError::InvalidPackageName { .. })),
            "{name}: {error:?}"
        );
        let local = format!("name = \"{name}\"\ngit_only = true");
        let plan = crate::fixture::plz_edited("name = \"app\"", &local)?;
        assert_eq!(refusal(&plan), None, "{name}");
    }
    let exact = "a".repeat(64);
    assert_eq!(
        refusal(&crate::fixture::plz_edited(
            "name = \"app\"",
            &format!("name = \"{exact}\"")
        )?),
        None
    );
    let mut inherited = files();
    with_metadata(
        &mut inherited,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("_y", "y/Cargo.toml")],
        )],
    );
    assert!(matches!(
        refusal(&inherited),
        Some(ReleaseError::InvalidPackageName { .. })
    ));
    Ok(())
}

#[test]
fn a_release_root_starting_with_a_dash_is_refused() {
    for root in ["-tools", "tools/-x"] {
        let mut files = files();
        files.release_toml = Some(format!("roots = [\"{root}\"]\n"));
        files
            .release_plz_toml
            .insert(root.to_owned(), PLZ.to_owned());
        let error = refusal(&files);
        let expected = root.starts_with('-');
        assert_eq!(
            matches!(error, Some(ReleaseError::UnsafeRoot { .. })),
            expected,
            "{root}: {error:?}"
        );
        if !expected {
            assert!(error.is_none(), "{root}: {error:?}");
        }
    }
}
