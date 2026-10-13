//! Immutable native restore provenance. Transported fields are not authentication.

use crate::{WorkspaceRoots, receipt_evidence::validate_workspace_paths};
use eyre::Result;
use mbx_cache_core::CacheDigest;
use serde::{Deserialize, Serialize};

pub(super) fn deserialize_required_lineage<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<ReceiptLineage>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<ReceiptLineage>::deserialize(deserializer)
}

/// The selected native snapshot owner frozen before a completed Cargo invocation.
///
/// This record describes a local successful restore. Importing it does not grant
/// a local capability or authenticate an external source. Only the native local
/// grant verifier can establish continuity for its exact destination roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLineage {
    /// Strict provenance codec version.
    pub version: u8,
    /// Immutable content-addressed original snapshot owner anchor.
    pub owner: CacheDigest,
    /// Verified attachment from which native restore selected the snapshot.
    pub selected_attachment: CacheDigest,
    /// Canonical digest of the selected complete native snapshot.
    pub selected_state: CacheDigest,
    /// Flat exact selected native CAS closure. References pin bytes only.
    pub selected_objects: Vec<CacheDigest>,
    /// Original owner roots, retained without normalization or reconstruction.
    pub origin: WorkspaceRoots,
    /// Exact logical workspace and Cargo roots resolved before the invocation.
    pub destination: WorkspaceRoots,
    /// Physical workspace and Cargo roots verified before the invocation.
    pub physical: WorkspaceRoots,
}

impl ReceiptLineage {
    /// Validate strict shape and root topology; this does not grant authority.
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            eyre::bail!("unsupported native receipt lineage version");
        }
        for digest in [&self.owner, &self.selected_attachment, &self.selected_state] {
            digest.validate()?;
        }
        let mut previous = None;
        for object in &self.selected_objects {
            object.validate()?;
            if previous.is_some_and(|value| value >= object) {
                eyre::bail!("native receipt lineage closure is not sorted and unique");
            }
            previous = Some(object);
        }
        if !self.selected_objects.contains(&self.owner)
            || !self.selected_objects.contains(&self.selected_attachment)
        {
            eyre::bail!("native receipt lineage closure omits its owner or selected attachment");
        }
        for roots in [&self.origin, &self.destination, &self.physical] {
            validate_workspace_paths(&roots.workspace_root, Some(&roots.cargo))?;
        }
        for roots in [&self.destination, &self.physical] {
            let relation = |left: &std::path::Path, right: &std::path::Path| {
                right
                    .strip_prefix(left)
                    .ok()
                    .map(std::path::Path::to_path_buf)
            };
            if relation(&self.origin.cargo.target_dir, &self.origin.cargo.build_dir)
                != relation(&roots.cargo.target_dir, &roots.cargo.build_dir)
                || relation(&self.origin.cargo.build_dir, &self.origin.cargo.target_dir)
                    != relation(&roots.cargo.build_dir, &roots.cargo.target_dir)
            {
                eyre::bail!("native receipt lineage has incompatible Cargo root topology");
            }
        }
        Ok(())
    }
}
