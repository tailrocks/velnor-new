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
pub(crate) const MISE_LOCK: &str = "[[tools.release-plz]]\nversion = \"0.4.3\"\n";

pub(crate) fn files() -> ReleaseFiles {
    ReleaseFiles {
        release_toml: None,
        release_plz_toml: [(".".to_owned(), PLZ.to_owned())].into(),
        mise_tomls: vec![MISE_TOML.to_owned()],
        cargo_configs: BTreeMap::new(),
        cargo_manifests: None,
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
