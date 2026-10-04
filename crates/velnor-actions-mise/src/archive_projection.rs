//! Source-only archive projection contract; no transport is currently qualified.
//!
//! Capability identity binds the inventory template before embedding literals.
//! Final generated-helper hashes remain external, avoiding a digest cycle.
//! This module performs no filesystem metadata transformation.

#[cfg(test)]
use velnor_actions_contract::compiled_source_sha256;

#[path = "archive_projection_identity.rs"]
mod identity;
use identity::{CapabilityTuple, framed_sha256};

/// Closed source authority, never loaded from JSON, environment or receipt bytes.
///
/// Private fields and the absent registry factory prevent shape-only admission.
/// A digest describes compatibility; it never authenticates a cache publisher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceArchiveProjection {
    tuple: CapabilityTuple,
    identity: String,
}

/// There is no immutable adapter pin or qualified Foundation runtime registry.
#[must_use]
pub fn qualified_source_archive_projection() -> Option<SourceArchiveProjection> {
    None
}

impl SourceArchiveProjection {
    /// Acyclic digest of the exact compiled source capability tuple.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Fixed Python literals for the shared algorithm owner's source compiler.
    ///
    /// This is descriptive source, with no loader, factory, transform or grant.
    /// The compiler must independently compare its final emitted helper digest.
    #[must_use]
    pub fn source_literals(&self) -> String {
        format!(
            "_ARCHIVE_PROJECTION_CAPABILITY = '{}'\n_ARCHIVE_PROJECTION_TEMPLATE = '{}'\n_ARCHIVE_PROJECTION_REVISION = '{}'\n",
            self.tuple.sha256(),
            self.tuple.inventory_template_sha256,
            self.tuple.transform_revision,
        )
    }

    /// Common payload purpose used before/after save, signed verification and health.
    ///
    /// Root context is ordered logical root evidence from the compiled mapping
    /// owner. Physical relocation paths and evidence roots are excluded. This
    /// hashing method cannot admit a mapping or grant runtime execution authority.
    #[must_use]
    pub fn payload_context_sha256(&self, logical_roots: &[&str]) -> String {
        let mut fields = vec![self.identity.as_bytes()];
        fields.extend(logical_roots.iter().map(|root| root.as_bytes()));
        framed_sha256("payload-v1", &fields)
    }

    #[cfg(test)]
    fn fixture(tuple: CapabilityTuple) -> Result<Self, &'static str> {
        tuple.validate()?;
        if tuple.inventory_template_sha256
            != crate::source_archive_inventory::source_template_sha256(
                crate::source_archive_inventory::InventorySourceProgram::Archive,
            )
        {
            return Err("inventory_template_sha256");
        }
        let identity = tuple.sha256();
        Ok(Self { tuple, identity })
    }
}

#[cfg(test)]
#[path = "archive_projection_tests.rs"]
mod tests;
