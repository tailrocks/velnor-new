//! Neutral identity inputs: the complete task-identity preimage.
//!
//! [`IdentityInputs`] carries every adapter fact the identity envelope
//! binds (unit identity, target, features, flags, drivers, environment,
//! declared inputs), converted wholesale from the adapter proposal.
//!
//! Why not [`TaskConfiguration`](velnor_actions_contract::canonical::TaskConfiguration)
//! directly (§4-step-4 reuse-or-justify): `TaskConfiguration` is the
//! serialized wire config block (7 fields under `deny_unknown_fields`
//! in the `task-execution-v1`/`task-report-v1` contracts); the preimage
//! spans task id, kind, dependencies, component, unit paths, and the
//! environment, none of which can extend that wire struct without
//! breaking compatibility with in-flight runs. `IdentityInputs` is the
//! neutral preimage record; the digest builder projects it plus the
//! node fields into the envelope.

use std::collections::BTreeMap;

use crate::graph::{check_sorted, check_sorted_unique};
use velnor_actions_contract::cachekey::validate_semantic_text;
use velnor_actions_contract::canonical::normalize_posix_path;
use velnor_actions_contract::errors::ContractError;

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
mod tests;
