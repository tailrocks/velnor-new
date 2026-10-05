//! Neutral identity inputs: the complete task-identity preimage.
//!
//! [`IdentityInputs`] carries every adapter fact the identity envelope
//! binds (unit identity, target, features, flags, drivers, environment,
//! declared inputs), converted wholesale from the adapter proposal.
//!
//! Why not [`TaskConfiguration`](crate::canonical::TaskConfiguration)
//! directly (§4-step-4 reuse-or-justify): `TaskConfiguration` is the
//! serialized wire config block (7 fields under `deny_unknown_fields`
//! in the `task-execution-v1`/`task-report-v1` contracts); the preimage
//! spans task id, kind, dependencies, component, unit paths, and the
//! environment, none of which can extend that wire struct without
//! breaking compatibility with in-flight runs. `IdentityInputs` is the
//! neutral preimage record; the digest builder projects it plus the
//! node fields into the envelope.

use std::collections::BTreeMap;

use crate::cachekey::validate_semantic_text;
use crate::canonical::normalize_posix_path;
use crate::errors::ContractError;
use crate::graph::{check_sorted, check_sorted_unique};

/// Complete neutral identity preimage for one proposed task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityInputs {
    /// Raw adapter unit id, verbatim (ownership joins only).
    pub unit_id: String,
    /// Stable unit key used in task ids and adapter metadata.
    pub unit_key: String,
    /// Repository-relative evidence path backing the unit.
    pub unit_path: String,
    /// Project root derived from `unit_path` (`.` for the repository root).
    pub project_root: String,
    /// Execution target (`host` or triple; validated downstream).
    pub target: String,
    /// Sorted enabled features.
    pub features: Vec<String>,
    /// Sorted extra fixed flags.
    pub flags: Vec<String>,
    /// Selected compile driver (adapter spelling).
    pub compile_driver: String,
    /// Selected test runner (adapter spelling).
    pub test_runner: String,
    /// Behavior-affecting environment contract.
    pub environment: BTreeMap<String, String>,
    /// Declared non-adapter task inputs (sorted, deduped).
    pub declared_inputs: Vec<String>,
    /// The unit may read undeclared inputs (conservative when true).
    pub undeclared_reads: bool,
}

impl IdentityInputs {
    /// Validate shapes and ordering (spellings stay adapter-owned).
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for malformed keys/paths, unordered
    /// lists, or empty driver spellings.
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_semantic_text("unit_key", &self.unit_key)?;
        normalize_posix_path(&self.unit_path)?;
        check_sorted(&self.features, "features")?;
        check_sorted(&self.flags, "flags")?;
        check_sorted_unique(&self.declared_inputs, "declared_inputs")?;
        validate_semantic_text("compile_driver", &self.compile_driver)?;
        validate_semantic_text("test_runner", &self.test_runner)?;
        Ok(())
    }
}

/// Stable component identity from a raw unit id plus evidence path.
///
/// Raw adapter ids are diagnostic-only: absolute checkout paths
/// embedded in `path+file://` or `registry+` qualifiers never enter an
/// identity. Plain ids pass through verbatim; qualified ids reduce to
/// their trailing `name@version` fragment; empty ids anchor to the
/// owning unit directory (or `workspace` at the root). This is the
/// single owner of component-id semantics.
#[must_use]
pub fn component_id_for_unit(unit_id: &str, unit_path: &str) -> String {
    if unit_id.is_empty() {
        let root = unit_path
            .rsplit_once('/')
            .map_or("", |(dir, _)| if dir.is_empty() { "" } else { dir });
        if root.is_empty() {
            return "workspace".to_owned();
        }
        return root.to_owned();
    }
    if let Some(fragment) = unit_id.rsplit('#').next()
        && fragment.contains('@')
        && unit_id.contains("://")
    {
        return fragment.to_owned();
    }
    unit_id.to_owned()
}

/// Project root for a unit evidence path; `.` for the repository root.
#[must_use]
pub fn project_root_for_unit_path(unit_path: &str) -> String {
    unit_path
        .rsplit_once('/')
        .map_or(".", |(dir, _)| if dir.is_empty() { "." } else { dir })
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Component mapping keeps every documented case byte-identical.
    #[test]
    fn component_ids_shed_checkout_paths() {
        assert_eq!(component_id_for_unit("demo", "Cargo.toml"), "demo");
        assert_eq!(
            component_id_for_unit("demo 0.1.0", "Cargo.toml"),
            "demo 0.1.0"
        );
        assert_eq!(
            component_id_for_unit("path+file:///Users/dev/proj#demo@0.1.0", "Cargo.toml"),
            "demo@0.1.0"
        );
        assert_eq!(
            component_id_for_unit(
                "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
                "Cargo.toml"
            ),
            "serde@1.0.0"
        );
        assert_eq!(component_id_for_unit("", "Cargo.toml"), "workspace");
        assert_eq!(component_id_for_unit("", "crates/a/Cargo.toml"), "crates/a");
    }

    /// Project roots anchor manifests; the repository root is `.`.
    #[test]
    fn project_roots_anchor_unit_paths() {
        assert_eq!(project_root_for_unit_path("Cargo.toml"), ".");
        assert_eq!(
            project_root_for_unit_path("crates/a/Cargo.toml"),
            "crates/a"
        );
    }

    /// Validation pins shapes without owning adapter spellings.
    #[test]
    fn validation_pins_shapes_not_spellings() {
        let valid = IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "root".to_owned(),
            unit_path: "Cargo.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: vec!["a".to_owned(), "b".to_owned()],
            flags: Vec::new(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        };
        assert!(valid.validate().is_ok());
        let unordered = IdentityInputs {
            features: vec!["b".to_owned(), "a".to_owned()],
            ..valid.clone()
        };
        assert!(unordered.validate().is_err());
        let bad_key = IdentityInputs {
            unit_key: String::new(),
            ..valid.clone()
        };
        assert!(bad_key.validate().is_err());
    }
}
