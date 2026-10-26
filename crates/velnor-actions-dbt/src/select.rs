// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::paths::is_under;

#[must_use]
pub fn affected_projects(projects: &[String], changed: &BTreeSet<String>) -> BTreeSet<String> {
    projects
        .iter()
        .filter(|dir| changed.iter().any(|path| is_under(path, dir)))
        .cloned()
        .collect()
}
