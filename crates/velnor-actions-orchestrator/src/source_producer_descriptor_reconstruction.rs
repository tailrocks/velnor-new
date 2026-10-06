//! Canonical descriptor reconstruction through the generating owner's policy.

use super::{MAX_JSON_BYTES, MODE, RustSourceDescriptor, archive_inputs, contract, safe_name};
use crate::OrchestratorError;
use crate::source_prep::SourceTransportAdmission;

impl RustSourceDescriptor {
    /// Reconstruct only a bounded, canonical descriptor accepted by source owners.
    pub(crate) fn from_hex(argument: &str) -> Result<Self, OrchestratorError> {
        if argument.is_empty() || argument.len() > MAX_JSON_BYTES * 2 || argument.len() % 2 != 0 {
            return Err(contract("rust_source_descriptor_hex_bound"));
        }
        let bytes: Result<Vec<u8>, OrchestratorError> = argument
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high =
                    hex_digit(pair[0]).ok_or_else(|| contract("rust_source_descriptor_hex"))?;
                let low =
                    hex_digit(pair[1]).ok_or_else(|| contract("rust_source_descriptor_hex"))?;
                Ok((high << 4) | low)
            })
            .collect();
        let bytes = bytes?;
        let descriptor: Self = serde_json::from_slice(&bytes).map_err(contract)?;
        if descriptor.json()? != bytes {
            return Err(contract("rust_source_descriptor_noncanonical"));
        }
        descriptor.qualify_captured()?;
        Ok(descriptor)
    }

    fn qualify_captured(&self) -> Result<(), OrchestratorError> {
        if self.schema != 1
            || self.rust_version != velnor_actions_mise::catalog::RUST_VERSION
            || self.target != velnor_actions_mise::catalog::RUST_TARGET_TRIPLE
            || self.roots.is_empty()
            || !strictly_sorted(&self.roots)
            || !strictly_sorted_paths(&self.manifests)
            || !strictly_sorted_paths(&self.locks)
        {
            return Err(contract("rust_source_descriptor_policy"));
        }
        velnor_actions_mise::catalog::validate_exact_version("rust", &self.rust_version)
            .map_err(contract)?;
        let admission = SourceTransportAdmission::from_captured(&self.roots, &self.locks, &[])
            .ok_or_else(|| contract("rust_source_descriptor_lock_policy"))?;
        let archives = archive_inputs(admission.locks())
            .ok_or_else(|| contract("rust_source_descriptor_archive_policy"))?;
        if archives.is_empty() || archives != self.archives {
            return Err(contract("rust_source_descriptor_archive_mismatch"));
        }
        super::super::manifest::qualify_captured(&self.roots, &self.manifests)?;
        self.qualify_selections()
    }

    fn qualify_selections(&self) -> Result<(), OrchestratorError> {
        let valid = match self.mode.as_str() {
            MODE => self.selections.is_empty(),
            "native-tree-selected-containing" => {
                !self.selections.is_empty()
                    && strictly_sorted(&self.selections)
                    && self.selections.iter().all(|selection| {
                        self.roots.contains(&selection.root)
                            && safe_name(&selection.package)
                            && strictly_sorted(&selection.features)
                            && selection.features.iter().all(|feature| safe_name(feature))
                            && (!selection.default_features || selection.features.is_empty())
                            && selection
                                .target
                                .as_deref()
                                .is_none_or(velnor_actions_contract::is_supported_target)
                    })
            }
            _ => false,
        };
        if !valid {
            return Err(contract("rust_source_descriptor_selection_policy"));
        }
        for selection in &self.selections {
            if !super::super::manifest::selected_package(
                &self.manifests,
                &selection.root,
                &selection.package,
            )? {
                return Err(contract("rust_source_descriptor_selected_package"));
            }
        }
        Ok(())
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn strictly_sorted_paths(values: &[(String, String)]) -> bool {
    !values.is_empty() && values.windows(2).all(|pair| pair[0].0 < pair[1].0)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "source_producer_descriptor_reconstruction_tests.rs"]
mod tests;
