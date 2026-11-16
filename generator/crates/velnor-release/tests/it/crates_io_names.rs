// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles};

use crate::fixture::{PLZ, TestResult, edit, files, metadata, refusal};

fn named(name: &str, publish: &str, with_metadata: bool) -> Result<ReleaseFiles, String> {
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        edit(PLZ, "name = \"app\"", &format!("name = \"{name}\""))?,
    );
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n{publish}"),
    );
    if with_metadata {
        files.rust_metadata = Some(metadata(&[("Cargo.toml", &[(name, "Cargo.toml")])]));
    }
    Ok(files)
}

fn invalid_name(files: &ReleaseFiles) -> bool {
    matches!(
        refusal(files),
        Some(ReleaseError::InvalidPackageName { .. })
    )
}

#[test]
fn a_package_cargo_does_not_publish_to_crates_io_keeps_the_local_name_rule() -> TestResult {
    for with_metadata in [false, true] {
        for publish in ["publish = false\n", "publish = []\n"] {
            let files = named("_x", publish, with_metadata)?;
            assert_eq!(refusal(&files), None, "{publish} {with_metadata}");
        }
        for publish in ["", "publish = true\n", "publish = [\"crates-io\"]\n"] {
            assert!(
                invalid_name(&named("_x", publish, with_metadata)?),
                "{publish}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_workspace_inherited_publish_false_keeps_the_local_name_rule() -> TestResult {
    let mut files = named("_x", "publish.workspace = true\n", true)?;
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        "[package]\nname = \"_x\"\nversion = \"0.1.0\"\npublish.workspace = true\n".to_owned(),
    );
    files.rust_metadata = Some(metadata(&[("Cargo.toml", &[("_x", "member/Cargo.toml")])]));
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace.package]\npublish = false\n".to_owned(),
    );
    assert_eq!(refusal(&files), None);
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace.package]\npublish = true\n".to_owned(),
    );
    assert!(invalid_name(&files));
    Ok(())
}

#[test]
fn windows_special_filenames_are_refused_for_publishing_packages() -> TestResult {
    for name in ["nul", "CON", "Aux", "prn", "com1", "COM9", "lpt1", "Lpt9"] {
        assert!(invalid_name(&named(name, "", false)?), "{name}");
        assert!(invalid_name(&named(name, "", true)?), "{name}");
        assert_eq!(
            refusal(&named(name, "publish = false\n", true)?),
            None,
            "{name}"
        );
    }
    for name in ["com0", "lpt10", "console", "nully", "com"] {
        assert_eq!(refusal(&named(name, "", true)?), None, "{name}");
    }
    Ok(())
}

#[test]
fn a_digit_leading_name_is_refused_for_a_crates_io_publishing_package() -> TestResult {
    for with_metadata in [false, true] {
        assert!(
            invalid_name(&named("1x", "", with_metadata)?),
            "{with_metadata}"
        );
    }
    Ok(())
}
