// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::RustStackConfig;
use crate::errors::ContractError;
use crate::workflow::cache_family::SliceId;

impl RustStackConfig {
    pub(super) fn validate_state_slices(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = BTreeSet::new();
        for (slice, packages) in &self.state_slices {
            let path = format!("stacks.rust.state_slices.{slice}");
            SliceId::parse(slice).map_err(|error| {
                ContractError::config(file, "stacks.rust.state_slices", error.to_string())
            })?;
            if packages.is_empty() {
                return Err(ContractError::config(file, path, "empty_state_slice"));
            }
            for package in packages {
                if !seen.insert(package.as_str()) {
                    return Err(ContractError::config(
                        file,
                        path,
                        format!("package_in_two_state_slices:{package}"),
                    ));
                }
            }
        }
        Ok(())
    }
}
