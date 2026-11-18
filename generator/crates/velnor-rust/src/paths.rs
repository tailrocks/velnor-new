// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RELEASE_PLZ_FILE, RepoPath};

use crate::roots::CARGO_LOCK;
use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML};

const CARGO_MANIFEST: &str = "Cargo.toml";

pub(crate) fn is_cargo_lock(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(CARGO_LOCK.as_str())
}

pub(crate) fn is_nested_toolchain(path: &RepoPath) -> bool {
    path.as_str().rsplit_once('/').is_some_and(|(_, name)| {
        name == TOOLCHAIN_PLAIN.as_str() || name == TOOLCHAIN_TOML.as_str()
    })
}

pub(crate) fn is_workflow_file(path: &RepoPath) -> bool {
    path.as_str()
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| {
            !name.contains('/')
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
}

pub(crate) fn is_release_plz_config(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(RELEASE_PLZ_FILE)
}

pub(crate) fn tracked_manifests(tracked: &BTreeSet<RepoPath>) -> impl Iterator<Item = &RepoPath> {
    tracked.iter().filter(|path| is_named(path, CARGO_MANIFEST))
}

pub(crate) fn is_named(path: &RepoPath, name: &str) -> bool {
    path.as_str() == name
        || path
            .as_str()
            .strip_suffix(name)
            .is_some_and(|rest| rest.ends_with('/'))
}
