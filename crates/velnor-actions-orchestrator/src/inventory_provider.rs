//! One Cargo inventory boundary for fresh and authenticated analysis.

use std::path::Path;

use crate::OrchestratorError;
use crate::analysis_inventory::ValidatedInventory;
use crate::inventory::Inventories;

/// Authenticated evidence is the only alternative to fresh Cargo execution.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InventoryProvider<'a> {
    /// Discover and qualify every selected workspace through pinned Cargo.
    FreshCargo,
    /// Fresh non-Rust analysis; discovering any Rust work demands setup.
    FreshWithoutCargo,
    /// Reuse a complete inventory authenticated against its remote producer.
    ValidatedInventory(&'a ValidatedInventory),
}

impl<'a> InventoryProvider<'a> {
    /// Return the authenticated proof, if this provider has one.
    pub(crate) const fn validated(self) -> Option<&'a ValidatedInventory> {
        match self {
            Self::FreshCargo | Self::FreshWithoutCargo => None,
            Self::ValidatedInventory(inventory) => Some(inventory),
        }
    }

    /// Bind cached evidence to the current effective file set and resolution.
    pub(crate) fn validate_current(
        self,
        root: &Path,
        files: &[String],
    ) -> Result<(), OrchestratorError> {
        if let Some(inventory) = self.validated() {
            inventory
                .validate_current(root, files)
                .map_err(|problem| OrchestratorError::NeedsCargo { problem })?;
        }
        Ok(())
    }

    /// Reproduce successful candidate outcomes without running Cargo.
    pub(crate) fn cached_inventories(
        self,
        manifests: &[String],
    ) -> Result<Option<Inventories>, OrchestratorError> {
        self.require_cargo_allowed(!manifests.is_empty())?;
        let Some(inventory) = self.validated() else {
            return Ok(None);
        };
        let mut outcomes = Vec::with_capacity(manifests.len());
        let mut records = Vec::with_capacity(manifests.len());
        for manifest in manifests {
            let record =
                inventory
                    .record_for(manifest)
                    .ok_or_else(|| OrchestratorError::NeedsCargo {
                        problem: format!("inventory_missing_manifest:{manifest}"),
                    })?;
            outcomes.push(velnor_actions_contract::CandidateOutcome {
                manifest: manifest.clone(),
                metadata_ok: true,
                diagnostic: None,
            });
            records.push((manifest.clone(), record));
        }
        Ok(Some((outcomes, records)))
    }

    /// Prevent Cargo from appearing after an earlier non-Rust admission check.
    pub(crate) fn require_cargo_allowed(
        self,
        has_rust_work: bool,
    ) -> Result<(), OrchestratorError> {
        if matches!(self, Self::FreshWithoutCargo) && has_rust_work {
            return Err(OrchestratorError::NeedsCargo {
                problem: "rust_discovered_after_no_cargo_admission".to_owned(),
            });
        }
        Ok(())
    }
}
