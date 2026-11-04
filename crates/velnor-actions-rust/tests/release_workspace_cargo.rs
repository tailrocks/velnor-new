// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, fs, path::Path, process::Command};
use velnor_actions_rust::{
    ReleaseError,
    metadata::parse_metadata_json,
    release_graph::{RegistryState, publication_graph},
    release_select::{ReleaseRequest, ReleaseScope, select_release_set},
};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    assert!(
        path.parent()
            .is_some_and(|parent| fs::create_dir_all(parent).is_ok())
    );
    assert!(fs::write(path, text).is_ok());
}

#[path = "release_workspace_cargo/registry.rs"]
mod registry;
#[path = "release_workspace_cargo/sources.rs"]
mod sources;

#[path = "release_workspace_cargo/version_presence.rs"]
mod version_presence;
