// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::{FamilyError, ReleaseError};

use crate::release_family_fixture::{generate, refused, tools_like, HELPER};

type TestResult = Result<(), Box<dyn Error>>;

fn family_error(config: &str) -> Result<FamilyError, Box<dyn Error>> {
    match refused(config)? {
        GenerateError::Release(ReleaseError::Family(error)) => Ok(error),
        other => Err(format!("unexpected error: {other}").into()),
    }
}

#[test]
fn a_family_key_without_family_tag_is_refused() -> TestResult {
    for (config, key) in [
        ("checksums_file = \"SUMS\"\n[[binaries]]\npackage = \"app\"\n", "checksums_file"),
        ("commit_file = true\n[[binaries]]\npackage = \"app\"\n", "commit_file"),
        ("attest = true\n[[binaries]]\npackage = \"app\"\n", "attest"),
        ("family_trigger = \"tag_push\"\n[[binaries]]\npackage = \"app\"\n", "family_trigger"),
    ] {
        assert_eq!(family_error(config)?, FamilyError::NeedsFamilyTag { key }, "{key}");
    }
    Ok(())
}

#[test]
fn a_binary_key_that_belongs_to_the_whole_family_names_the_top_level_key() -> TestResult {
    for (key, instead) in [
        ("attest = true", "attest"),
        ("commit_file = true", "commit_file"),
        ("member_digests = \"sha256\"", "checksums_file"),
    ] {
        let config = format!(
            "family_tag = \"tools-v{{{{ version }}}}\"\n[[binaries]]\npackage = \"app\"\n{key}\n"
        );
        let name = key.split(' ').next().unwrap_or_default();
        match family_error(&config)? {
            FamilyError::BinaryKeyBelongsToFamily { key, instead: found } => {
                assert_eq!((key, found), (name, instead));
            }
            other => return Err(format!("unexpected: {other}").into()),
        }
    }
    Ok(())
}

#[test]
fn checksums_file_must_be_a_plain_new_name_over_archives() -> TestResult {
    for name in ["SHA256SUMS", "COMMIT", "a/b", "-x", ".x", "a b", "", "${{ github.ref }}"] {
        let config = tools_like(&format!("checksums_file = {name:?}\n"));
        assert!(refused(&config).is_ok(), "{name:?}");
    }
    let raw = "family_tag = \"tools-v{{ version }}\"\nchecksums_file = \"BINARY-SHA256SUMS\"\n[[binaries]]\npackage = \"app\"\nformat = \"raw\"\n";
    assert!(matches!(
        family_error(raw)?,
        FamilyError::ChecksumsNeedArchives { .. }
    ));
    Ok(())
}

#[test]
fn smoke_on_an_archive_binary_is_accepted() -> TestResult {
    generate(HELPER, &tools_like(""))?;
    Ok(())
}
