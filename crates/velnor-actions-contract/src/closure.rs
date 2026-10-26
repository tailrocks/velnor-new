// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ContractError;
use crate::canonical::digest_b3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    Known { digest: String },

    AbsentProven { evidence: String },

    GuardedExternally { guard: String },

    Unknown { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInputClosure {
    pub task_id: String,

    pub inputs: BTreeMap<String, Provenance>,
}

impl TaskInputClosure {
    #[must_use]
    pub fn unknown_inputs(&self) -> Vec<&str> {
        self.inputs
            .iter()
            .filter(|(_, p)| matches!(p, Provenance::Unknown { .. }))
            .map(|(n, _)| n.as_str())
            .collect()
    }

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

#[derive(Debug)]
pub struct ClosureBuilder {
    inputs: BTreeMap<String, Provenance>,
}

impl ClosureBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn input(mut self, name: &str, provenance: Provenance) -> Self {
        self.inputs.insert(name.to_owned(), provenance);
        self
    }

    #[must_use]
    pub fn value(self, name: &str, value: &str) -> Self {
        self.input(name, Self::known(digest_b3(value.as_bytes())))
    }

    #[must_use]
    pub fn digest(self, name: &str, digest: &str) -> Self {
        self.input(name, Self::known(digest.to_owned()))
    }

    #[must_use]
    pub fn build(self, task_id: &str) -> TaskInputClosure {
        TaskInputClosure {
            task_id: task_id.to_owned(),
            inputs: self.inputs,
        }
    }

    fn known(digest: String) -> Provenance {
        Provenance::Known { digest }
    }
}

impl Default for ClosureBuilder {
    fn default() -> Self {
        Self::new()
    }
}
