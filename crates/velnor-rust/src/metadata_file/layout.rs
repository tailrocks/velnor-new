// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoPath, RustMetadata};

use crate::sha256::sha256_hex;

const DISCOVERED_FILES: &[&str] = &["build.rs", "src/lib.rs", "src/main.rs"];
const DISCOVERED_FOLDERS: &[&str] = &["src/bin", "tests", "examples", "benches"];

fn discovered(relative: &str) -> bool {
    DISCOVERED_FILES.contains(&relative)
        || DISCOVERED_FOLDERS.iter().any(|folder| {
            relative
                .strip_prefix(folder)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|rest| {
                    rest.strip_suffix(".rs")
                        .is_some_and(|name| !name.contains('/'))
                        || rest
                            .strip_suffix("/main.rs")
                            .is_some_and(|name| !name.contains('/'))
                })
        })
}

pub(super) fn layout<'a>(
    tracked: &BTreeSet<RepoPath>,
    metadata: impl Iterator<Item = &'a RustMetadata>,
) -> String {
    let directories: BTreeSet<String> = metadata
        .flat_map(|metadata| &metadata.members)
        .map(|member| {
            member
                .manifest
                .parent()
                .map_or_else(String::new, |d| format!("{d}/"))
        })
        .collect();
    let found: Vec<&str> = tracked
        .iter()
        .map(RepoPath::as_str)
        .filter(|path| {
            directories.iter().any(|directory| {
                path.strip_prefix(directory.as_str())
                    .is_some_and(discovered)
            })
        })
        .collect();
    sha256_hex(found.join("\n").as_bytes())
}
