// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RustRoot};

use crate::roots::is_cargo_config;
use crate::weaken::{is_manifest, is_nextest_config};

pub(crate) fn reaches_a_check(file: &RepoPath, roots: &[RustRoot]) -> bool {
    if is_manifest(file) {
        return roots.iter().any(|root| {
            root.manifest == *file || root.members.iter().any(|member| member.manifest == *file)
        });
    }
    if is_cargo_config(file) {
        let folder = config_folder(file, ".cargo/");
        return roots.iter().any(|root| {
            let own = root_folder(root);
            folder.is_empty() || own == folder || own.starts_with(&format!("{folder}/"))
        });
    }
    if is_nextest_config(file) {
        let folder = config_folder(file, ".config/nextest.toml");
        return roots.iter().any(|root| root_folder(root) == folder);
    }
    true
}

fn root_folder(root: &RustRoot) -> String {
    root.manifest
        .parent()
        .map_or_default(|parent| parent.to_string())
}

fn config_folder(file: &RepoPath, marker: &str) -> String {
    let text = file.as_str();
    text.rfind(marker)
        .map_or_default(|at| text[..at].trim_end_matches('/'))
        .to_owned()
}
