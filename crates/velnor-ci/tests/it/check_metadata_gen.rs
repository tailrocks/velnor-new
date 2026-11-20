// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::io::{Error as IoError, check_metadata, write_metadata};
use velnor_rust::MetadataFileError;

type TestResult = Result<(), Box<dyn Error>>;

const RECORDED: &str = ".velnor/rust-metadata.json";
const LOCK: &str = ".velnor/rust-ci.lock";
const SAMPLE_LOCK: &str =
    include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

struct Checkout(PathBuf);

impl Checkout {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        Self::pinned(name, None)
    }

    fn pinned(name: &str, helper: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let base = crate::scratch_path("velnor-gen-check", name);
        if base.exists() {
            fs::remove_dir_all(&base)?;
        }
        fs::create_dir_all(&base)?;
        let checkout = Self(fs::canonicalize(base)?);
        checkout.git(&["init", "--quiet"])?;
        checkout.write(
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )?;
        checkout.write("src/lib.rs", "")?;
        if let Some(helper) = helper {
            checkout.write(LOCK, &SAMPLE_LOCK.replace("0.1.81", helper))?;
        }
        checkout.git(&["add", "."])?;
        write_metadata(&checkout.0)?;
        checkout.git(&["add", "."])?;
        Ok(checkout)
    }

    fn git(&self, arguments: &[&str]) -> TestResult {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(&self.0)
            .status()?;
        status.success().then_some(()).ok_or("git failed".into())
    }

    fn write(&self, path: &str, text: &str) -> TestResult {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(file, text)?;
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn differs(result: &Result<(), IoError>) -> bool {
    matches!(
        result,
        Err(IoError::MetadataFile(MetadataFileError::Differs(_)))
    )
}

#[test]
fn a_file_velnor_gen_just_wrote_passes() -> TestResult {
    let checkout = Checkout::new("fresh")?;
    check_metadata(checkout.path())?;
    Ok(())
}

#[test]
fn a_hand_edited_file_with_matching_hashes_fails() -> TestResult {
    let checkout = Checkout::new("edited")?;
    let text = fs::read_to_string(checkout.path().join(RECORDED))?;
    assert!(text.contains("\"tests\": true"));
    checkout.write(
        RECORDED,
        &text.replace("\"tests\": true", "\"tests\": false"),
    )?;
    let result = check_metadata(checkout.path());
    assert!(differs(&result), "hand edit passed");
    Ok(())
}

#[test]
fn a_new_auto_discovered_target_fails_although_no_manifest_changed() -> TestResult {
    for (name, path) in [
        ("test", "tests/x.rs"),
        ("build", "build.rs"),
        ("binary", "src/main.rs"),
    ] {
        let checkout = Checkout::new(name)?;
        checkout.write(path, "fn main() {}\n")?;
        checkout.git(&["add", "."])?;
        let result = check_metadata(checkout.path());
        assert!(
            matches!(
                result,
                Err(IoError::MetadataFile(MetadataFileError::Layout))
            ),
            "{path} was not noticed"
        );
    }
    Ok(())
}

#[test]
fn the_version_is_recorded_and_checked() -> TestResult {
    let checkout = Checkout::pinned("0.1.81", Some("0.1.81"))?;
    let text = fs::read_to_string(checkout.path().join(RECORDED))?;
    assert!(text.contains("\"version\""));
    check_metadata(checkout.path())?;
    Ok(())
}
