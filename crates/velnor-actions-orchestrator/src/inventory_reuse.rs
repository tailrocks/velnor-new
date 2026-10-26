// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_rust::WorkspaceRecord;

#[derive(Debug, Default)]
pub(crate) struct MemberIndex {
    slots: BTreeMap<String, usize>,
}

impl MemberIndex {
    pub(crate) fn insert(&mut self, slot: usize, record: &WorkspaceRecord) {
        for package in &record.packages {
            if package.in_workspace && !package.external {
                self.slots.entry(package.manifest.clone()).or_insert(slot);
            }
        }
    }

    pub(crate) fn reuse_for(
        &self,
        root: &Path,
        manifest: &str,
        inventories: &[(String, WorkspaceRecord)],
    ) -> Option<WorkspaceRecord> {
        let slot = *self.slots.get(manifest)?;
        if declares_workspace_root(&root.join(manifest)) {
            return None;
        }
        inventories.get(slot).map(|(_, record)| record.clone())
    }
}

fn declares_workspace_root(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    toml::from_str::<toml::Table>(&text).is_ok_and(|table| table.contains_key("workspace"))
}
