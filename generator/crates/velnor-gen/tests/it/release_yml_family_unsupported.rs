// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_release::{FamilyError, ReleaseError};
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, release_view};

type TestResult = Result<(), Box<dyn Error>>;

const FAMILY: &str = "family_tag = \"tools-v{{ version }}\"\n";
const RELEASE_PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npr_branch_prefix = \"release-plz-app-\"\ngit_only = true\n\n[[package]]\nname = \"app\"\nrelease = false\n";

fn generate(binaries: &str) -> Result<(), Box<dyn Error>> {
    let config = format!("{FAMILY}{binaries}");
    let mut view = release_view(&[APP], Some(&config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", "0.1.83").into_bytes());
    view.files.insert(
        RepoPath::new("release-plz.toml")?,
        RELEASE_PLZ.as_bytes().to_vec(),
    );
    match expected_files(&view) {
        Ok(_) => Ok(()),
        Err(error) => Err(Box::new(error)),
    }
}

fn refused_key(binaries: &str) -> Result<&'static str, Box<dyn Error>> {
    let Err(error) = generate(binaries) else {
        return Err("the family release accepted a key it cannot publish".into());
    };
    match error.downcast::<GenerateError>().map(|error| *error) {
        Ok(GenerateError::Release(ReleaseError::Family(FamilyError::Unsupported { key }))) => {
            Ok(key)
        }
        Ok(other) => Err(format!("unexpected error: {other}").into()),
        Err(other) => Err(other),
    }
}

#[test]
fn a_family_release_of_one_plain_binary_still_renders() -> TestResult {
    generate("[[binaries]]\npackage = \"app\"\n")
}

#[test]
fn a_family_release_refuses_the_keys_it_cannot_publish_yet() -> TestResult {
    for (binaries, key) in [
        (
            "[[binaries]]\npackage = \"app\"\nasset_name = \"{{ bin }}-{{ platform }}\"\n",
            "asset_name",
        ),
        (
            "[[binaries]]\npackage = \"app\"\nbins = [{ bin = \"rg\" }]\n",
            "bins",
        ),
        (
            "[[binaries]]\npackage = \"app\"\ninclude = [\"LICENSE\"]\n",
            "include",
        ),
        ("[[binaries]]\npackage = \"app\"\nattest = true\n", "attest"),
    ] {
        assert_eq!(refused_key(binaries)?, key);
    }
    Ok(())
}

#[test]
fn a_family_release_refuses_pre_publish_checks() -> TestResult {
    let binaries = "[[binaries]]\npackage = \"app\"\n[[checks]]\nname = \"Qualify\"\nrun = \"bash qualify.sh\"\n";
    assert_eq!(refused_key(binaries)?, "[[checks]]");
    Ok(())
}
