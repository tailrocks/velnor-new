// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub write: bool,
    /// Audit only the Cargo root in this directory, from its own target directory.
    pub root: Option<String>,
    pub max_directory_entries: usize,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            write: false,
            root: None,
            max_directory_entries: super::inventory::LIMIT,
        }
    }
}
