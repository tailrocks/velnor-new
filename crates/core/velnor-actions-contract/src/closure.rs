//! Complete first-party input closures with explicit unknowns.
//!
//! Unknown inputs forbid reuse and coverage; absence and ignorance
//! never collapse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ContractError;
use crate::canonical::digest_b3;

/// Provenance of one semantic input: knowledge, never assumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    /// Content digest bound.
    Known {
        /// BLAKE3 digest of the bound content.
        digest: String,
    },
    /// Proven absent, with evidence.
    AbsentProven {
        /// Evidence the input is absent.
        evidence: String,
    },
    /// Guarded by an external verifier.
    GuardedExternally {
        /// External verifier guarding the input.
        guard: String,
    },
    /// Unresolved, with reason; blocks reuse and coverage.
    Unknown {
        /// Reason the input stays unresolved.
        reason: String,
    },
}

/// Complete first-party input closure for one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInputClosure {
    /// Stable task ID.
    pub task_id: String,
    /// Named input provenances, sorted.
    pub inputs: BTreeMap<String, Provenance>,
}

impl TaskInputClosure {
    /// Names of explicitly unknown inputs, sorted (`BTreeMap` order).
    #[must_use]
    pub fn unknown_inputs(&self) -> Vec<&str> {
        self.inputs
            .iter()
            .filter(|(_, p)| matches!(p, Provenance::Unknown { .. }))
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// Refuse reuse and coverage while any input is explicitly unknown.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] naming the unknown inputs.
    pub fn verify_complete(&self) -> Result<(), ContractError> {
        let unknown = self.unknown_inputs();
        if !unknown.is_empty() {
            return Err(ContractError::identity(
                "input_closure",
                format!("incomplete_inputs:{}", unknown.join(",")),
            ));
        }
        Ok(())
    }
}

/// Builder for [`TaskInputClosure`] over one task.
#[derive(Debug)]
pub struct ClosureBuilder {
    inputs: BTreeMap<String, Provenance>,
}

impl ClosureBuilder {
    /// Empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: BTreeMap::new(),
        }
    }

    /// Record one named input provenance.
    #[must_use]
    pub fn input(mut self, name: &str, provenance: Provenance) -> Self {
        self.inputs.insert(name.to_owned(), provenance);
        self
    }

    /// Record a value-bound input (features, target, profile, argv).
    #[must_use]
    pub fn value(self, name: &str, value: &str) -> Self {
        self.input(name, Self::known(digest_b3(value.as_bytes())))
    }

    /// Record a precomputed digest input (graph, toolchain, platform).
    #[must_use]
    pub fn digest(self, name: &str, digest: &str) -> Self {
        self.input(name, Self::known(digest.to_owned()))
    }

    /// Build the closure for `task_id`.
    #[must_use]
    pub fn build(self, task_id: &str) -> TaskInputClosure {
        TaskInputClosure {
            task_id: task_id.to_owned(),
            inputs: self.inputs,
        }
    }

    /// Known-content provenance.
    fn known(digest: String) -> Provenance {
        Provenance::Known { digest }
    }
}

impl Default for ClosureBuilder {
    fn default() -> Self {
        Self::new()
    }
}
