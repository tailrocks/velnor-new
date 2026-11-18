// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

use crate::fixture::{PLZ, TestResult, edit, files, refusal, with_metadata};

fn listing(extra: &str) -> Result<String, String> {
    edit(PLZ, "name = \"app\"\n", &format!("name = \"app\"\n{extra}"))
}

fn rooted(plz: &str, members: &[(&str, &str)]) -> ReleaseFiles {
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), plz.to_owned());
    with_metadata(&mut files, &[("Cargo.toml", members)]);
    files
}

#[test]
fn an_inherited_member_name_that_recording_rejects_is_refused_at_generation() -> TestResult {
    let members = |name: &'static str| [("app", "Cargo.toml"), (name, "other/Cargo.toml")];
    let plz = listing("")?;
    let error = refusal(&rooted(&plz, &members("f\u{fc}o")));
    assert!(
        matches!(&error, Some(ReleaseError::InvalidPackageName { package, .. }) if package == "f\u{fc}o"),
        "{error:?}"
    );
    assert_eq!(refusal(&rooted(&plz, &members("foo"))), None);
    Ok(())
}

#[test]
fn an_unlisted_member_tag_is_compared_with_every_effective_tag_of_the_root() -> TestResult {
    let members = [("app", "Cargo.toml"), ("other", "other/Cargo.toml")];
    let stolen = listing("git_tag_name = \"other-v{{ version }}\"\n")?;
    let error = refusal(&rooted(&stolen, &members));
    assert!(
        matches!(
            &error,
            Some(ReleaseError::DuplicateTagPattern { .. } | ReleaseError::TagsMayCollide { .. })
        ),
        "{error:?}"
    );
    let apart = listing("git_tag_name = \"app-v{{ version }}\"\n")?;
    assert_eq!(refusal(&rooted(&apart, &members)), None);
    Ok(())
}

#[test]
fn a_binary_of_a_workspace_inherited_member_takes_the_instantiated_workspace_tag() -> TestResult {
    let mut files = rooted(PLZ, &[("app", "Cargo.toml"), ("cli", "cli/Cargo.toml")]);
    files.release_toml = Some("[[binaries]]\npackage = \"cli\"\n".to_owned());
    let plan = ReleasePlan::read(&files)?;
    assert_eq!(plan.binaries[0].root, ".");
    assert_eq!(plan.binaries[0].tag_pattern.as_str(), "cli-v{{ version }}");
    files.release_toml = Some("[[binaries]]\npackage = \"ghost\"\n".to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::BinaryNotReleased { .. })
    ));
    Ok(())
}

#[test]
fn a_workspace_default_makes_a_root_publish_only_when_an_unlisted_member_inherits_it() -> TestResult
{
    let plz = listing("git_only = true\n")?;
    let mut files = rooted(&plz, &[("app", "Cargo.toml")]);
    assert!(!ReleasePlan::read(&files)?.roots[0].publishes_to_crates_io);
    with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("lib", "lib/Cargo.toml")],
        )],
    );
    assert!(ReleasePlan::read(&files)?.roots[0].publishes_to_crates_io);
    Ok(())
}
