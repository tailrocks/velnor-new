// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::cachekey::validate_semantic_text;
use crate::canonical::normalize_posix_path;
use crate::errors::ContractError;
use crate::graph::{check_sorted, check_sorted_unique};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityInputs {
    pub unit_id: String,

    pub unit_key: String,

    pub unit_path: String,

    pub project_root: String,

    pub target: String,

    pub features: Vec<String>,

    pub flags: Vec<String>,

    pub compile_driver: String,

    pub test_runner: String,

    pub environment: BTreeMap<String, String>,

    pub declared_inputs: Vec<String>,

    pub undeclared_reads: bool,
}

impl IdentityInputs {
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

#[must_use]
pub fn project_root_for_unit_path(unit_path: &str) -> String {
    unit_path
        .rsplit_once('/')
        .map_or(".", |(dir, _)| if dir.is_empty() { "." } else { dir })
        .to_owned()
}
