//! Workspace-record reuse across candidate manifests.
//!
//! After the first fetch, member manifests reuse the already-parsed record
//! instead of spawning another `cargo metadata` subprocess. A manifest
//! declaring its own `[workspace]` root is never reused: it owns a
//! distinct workspace with its own inventory.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_rust_core::WorkspaceRecord;

/// Manifest-to-slot index; each record indexed once, lookups `O(log n)`.
#[derive(Debug, Default)]
pub(crate) struct MemberIndex {
    slots: BTreeMap<String, usize>,
}

impl MemberIndex {
    /// Index one fetched record's in-workspace member manifests.
    pub(crate) fn insert(&mut self, slot: usize, record: &WorkspaceRecord) {
        for package in &record.packages {
            if package.in_workspace && !package.external {
                self.slots.entry(package.manifest.clone()).or_insert(slot);
            }
        }
    }

    /// Reuse a validated member record; `None` fetches fresh. Never reuses a
    /// manifest declaring its own `[workspace]` root.
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

/// True when the manifest parses with a top-level `[workspace]` table.
/// Unreadable files read as empty: the fresh fetch reports the real error.
fn declares_workspace_root(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    toml::from_str::<toml::Table>(&text).is_ok_and(|table| table.contains_key("workspace"))
}
