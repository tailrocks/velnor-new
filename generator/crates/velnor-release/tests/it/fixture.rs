// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{PackageName, RepoPath, RustMetadata, RustPackage};
use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) const PLZ: &str = r#"
[workspace]
git_tag_name = "{{ package }}-v{{ version }}"
git_release_draft = true
release_always = false
semver_check = false
publish_no_verify = true
publish_timeout = "5m"

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
        cargo_manifest_texts: BTreeMap::new(),
        bin_targets: None,
        rust_metadata: None,
        workflow_names: None,
        push_workflows: None,
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

pub(crate) fn metadata(workspaces: &[(&str, &[(&str, &str)])]) -> BTreeMap<RepoPath, RustMetadata> {
    let package = |(name, manifest): &(&str, &str)| RustPackage {
        name: PackageName::new((*name).to_owned()).unwrap_or_else(|e| unreachable!("{e}")),
        version: Some("0.1.0".to_owned()),
        manifest: RepoPath::new(*manifest).unwrap_or_else(|e| unreachable!("{e}")),
        dependencies: Vec::new(),
        targets: Vec::new(),
    };
    workspaces
        .iter()
        .map(|(workspace, members)| {
            (
                RepoPath::new(*workspace).unwrap_or_else(|e| unreachable!("{e}")),
                RustMetadata {
                    members: members.iter().map(package).collect(),
                },
            )
        })
        .collect()
}

pub(crate) fn with_metadata(files: &mut ReleaseFiles, workspaces: &[(&str, &[(&str, &str)])]) {
    for (workspace, members) in workspaces {
        for path in std::iter::once(workspace).chain(members.iter().map(|(_, path)| path)) {
            files
                .cargo_manifest_texts
                .entry((*path).to_owned())
                .or_insert_with(|| "[package]\nname = \"x\"\nversion = \"0.1.0\"\n".to_owned());
        }
    }
    files.rust_metadata = Some(metadata(workspaces));
}
