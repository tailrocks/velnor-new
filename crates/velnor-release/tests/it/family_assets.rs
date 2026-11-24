// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{FamilyError, ReleaseError, ReleaseFiles};

use crate::fixture::{TestResult, plz_edited, refusal};

const FAMILY: &str = "family_tag = \"tools-v{{ version }}\"\n";

fn family(binaries: &str) -> Result<ReleaseFiles, String> {
    let mut files = plz_edited(
        "name = \"app\"",
        "name = \"app\"\nrelease = false\n\n[[package]]\nname = \"tool\"\nrelease = false",
    )?;
    files.release_toml = Some(format!("{FAMILY}{binaries}"));
    Ok(files)
}

fn collision(binaries: &str) -> Result<Option<String>, String> {
    Ok(match refusal(&family(binaries)?) {
        Some(ReleaseError::Family(FamilyError::AssetNameCollision { file, .. })) => Some(file),
        Some(other) => return Err(format!("unexpected refusal: {other}")),
        None => None,
    })
}

#[test]
fn two_family_binaries_with_one_asset_name_template_are_refused() -> TestResult {
    let file = collision(
        "[[binaries]]\npackage = \"app\"\nasset_name = \"dist-{{ platform }}\"\n\n[[binaries]]\npackage = \"tool\"\nasset_name = \"dist-{{ platform }}\"\n",
    )?;
    assert_eq!(file.as_deref(), Some("dist-linux-x64.tar.gz"));
    Ok(())
}

#[test]
fn two_family_binaries_with_one_bin_name_are_refused() -> TestResult {
    let file = collision(
        "[[binaries]]\npackage = \"app\"\nbin = \"cli\"\n\n[[binaries]]\npackage = \"tool\"\nbin = \"cli\"\n",
    )?;
    assert_eq!(
        file.as_deref(),
        Some("cli-x86_64-unknown-linux-musl.tar.gz")
    );
    Ok(())
}

#[test]
fn raw_family_binaries_collide_on_their_shared_file_names() -> TestResult {
    let file = collision(
        "[[binaries]]\npackage = \"app\"\nbin = \"cli\"\nformat = \"raw\"\n\n[[binaries]]\npackage = \"tool\"\nbin = \"other\"\nformat = \"raw\"\nbins = [{ bin = \"cli\" }]\n",
    )?;
    assert_eq!(file.as_deref(), Some("cli-linux-x64.sha256"));
    Ok(())
}

#[test]
fn family_binaries_with_distinct_file_names_are_accepted() -> TestResult {
    assert_eq!(
        collision("[[binaries]]\npackage = \"app\"\n\n[[binaries]]\npackage = \"tool\"\n")?,
        None
    );
    assert_eq!(
        collision(
            "[[binaries]]\npackage = \"app\"\nbin = \"cli\"\nplatforms = [\"linux-x64\"]\n\n[[binaries]]\npackage = \"tool\"\nbin = \"cli\"\nplatforms = [\"linux-x64\"]\ntarget = \"gnu\"\n"
        )?,
        None
    );
    assert_eq!(
        collision(
            "[[binaries]]\npackage = \"app\"\nasset_name = \"{{ bin }}-{{ platform }}\"\n\n[[binaries]]\npackage = \"tool\"\nasset_name = \"{{ bin }}-{{ platform }}\"\n"
        )?,
        None
    );
    Ok(())
}
