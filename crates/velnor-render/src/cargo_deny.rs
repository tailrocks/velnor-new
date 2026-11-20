// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DenyCheck, RepoPath};

use crate::rules::Value;

pub(crate) fn deny(
    manifest: &RepoPath,
    config: Option<&RepoPath>,
    checks: Option<&[DenyCheck]>,
) -> (String, String, Vec<(String, Value)>) {
    let directory = manifest.parent();
    let name = directory.as_ref().map_or_else(
        || "Dependency policy".to_owned(),
        |directory| format!("Dependency policy ({directory})"),
    );
    let manifest_arg = directory.map_or_else(String::new, |_| {
        format!(
            " --manifest-path '{}'",
            manifest.as_str().replace('\'', "'\\''")
        )
    });
    let config_arg = config.map_or_else(String::new, |path| {
        format!(" --config '{}'", path.as_str().replace('\'', "'\\''"))
    });
    let checks_arg: String = checks
        .unwrap_or_default()
        .iter()
        .map(|check| [" ", check.name()].concat())
        .collect();
    (
        name,
        format!("cargo-deny{manifest_arg}{config_arg} --locked check{checks_arg}"),
        vec![],
    )
}
