// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) const PLZ: &str = r#"
[workspace]
git_tag_name = "{{ package }}-v{{ version }}"
git_release_draft = true
release_always = false
semver_check = false
publish_no_verify = true

[[package]]
name = "app"
"#;

pub(crate) const MISE_TOML: &str = "[tools]\nrelease-plz = \"0.4.3\"\n";
pub(crate) const RELEASE_PLZ_SHA256: &str =
    "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
pub(crate) const RELEASE_PLZ_URL: &str = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz";
pub(crate) const MISE_LOCK: &str = "[[tools.release-plz]]\nversion = \"0.4.3\"\n\n[tools.release-plz.\"platforms.linux-x64\"]\nchecksum = \"sha256:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90\"\nurl = \"https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz-x86_64-unknown-linux-gnu.tar.gz\"\n";

pub(crate) fn locked(key: &str, version: &str) -> String {
    format!(
        "[[tools.\"{key}\"]]\nversion = \"{version}\"\n\n[tools.\"{key}\".\"platforms.linux-x64\"]\nchecksum = \"sha256:{RELEASE_PLZ_SHA256}\"\nurl = \"{RELEASE_PLZ_URL}\"\n"
    )
}

pub(crate) fn files() -> ReleaseFiles {
    ReleaseFiles {
        release_toml: None,
        release_plz_toml: [(".".to_owned(), PLZ.to_owned())].into(),
        mise_tomls: vec![MISE_TOML.to_owned()],
        cargo_configs: BTreeMap::new(),
        toolchain_files: BTreeMap::new(),
        symlinks: std::collections::BTreeSet::new(),
        cargo_manifests: None,
        bin_targets: None,
        root_members: None,
        workflow_names: None,
        mise_lock: MISE_LOCK.to_owned(),
    }
}

pub(crate) fn edit(text: &str, from: &str, to: &str) -> Result<String, String> {
    if text.contains(from) {
        Ok(text.replacen(from, to, 1))
    } else {
        Err(format!("fixture lacks {from:?}"))
    }
}

pub(crate) fn plz_edited(from: &str, to: &str) -> Result<ReleaseFiles, String> {
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), edit(PLZ, from, to)?);
    Ok(files)
}

pub(crate) fn refusal(files: &ReleaseFiles) -> Option<ReleaseError> {
    ReleasePlan::read(files).err()
}

pub(crate) fn plz_refusal_of(from: &str, to: &str) -> Result<Option<ReleaseError>, String> {
    Ok(refusal(&plz_edited(from, to)?))
}
