// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::validate_digest;

use crate::cache::verify_artifact_digest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreObservation {
    pub entry_path: String,
    pub entry_bytes: Vec<u8>,
    pub expected_digest: String,
    pub expected_compat: String,
    pub observed_compat: String,
    pub expected_owner: String,
    pub observed_owner: String,
    pub expected_inputs: String,
    pub observed_inputs: String,
}

impl RestoreObservation {
    #[must_use]
    pub fn checks(&self) -> [bool; 5] {
        let present = !self.entry_path.trim().is_empty();
        let digest_ok = verify_artifact_digest(&self.entry_bytes, &self.expected_digest).is_ok();
        let compat_ok = digests_equal(&self.expected_compat, &self.observed_compat);
        let owner_ok =
            !self.observed_owner.is_empty() && self.observed_owner == self.expected_owner;
        let inputs_ok = digests_equal(&self.expected_inputs, &self.observed_inputs);
        [present, digest_ok, compat_ok, owner_ok, inputs_ok]
    }
}

fn digests_equal(expected: &str, observed: &str) -> bool {
    validate_digest(expected).is_ok() && expected == observed
}

pub fn verify_provider_restore(obs: &RestoreObservation) -> Result<(), &'static str> {
    classify_restore(obs)
}

pub fn classify_restore(obs: &RestoreObservation) -> Result<(), &'static str> {
    let reasons = [
        "no_entry",
        "cache_corrupt",
        "compatibility_mismatch",
        "trust_scope_mismatch",
        "input_digest_mismatch",
    ];
    for (index, ok) in obs.checks().iter().enumerate() {
        if !ok {
            return Err(reasons[index]);
        }
    }
    Ok(())
}

#[must_use]
pub fn output_bytes_complete(bytes: &[u8]) -> bool {
    !bytes.is_empty()
}
