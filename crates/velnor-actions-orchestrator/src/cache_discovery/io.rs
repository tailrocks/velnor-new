// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;

use super::types::{
    DiscoveryError, DiscoveryV1, MAX_DISCOVERY_JSON_BYTES, contract_error, discovery_error,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreObservation {
    pub intended_key: String,
    pub matched_key: String,
    pub restored_compatibility: Digest,
    pub expected_compatibility: Digest,
    pub restored_coverage: Digest,
    pub expected_coverage: Digest,
    pub restored_manifest: Digest,
    pub intended_manifest: Digest,
}

pub fn verify_restore(observation: &RestoreObservation) -> Result<(), DiscoveryError> {
    if observation.intended_key.is_empty() || observation.matched_key.is_empty() {
        return Err(discovery_error("restored_key_empty"));
    }
    if observation.matched_key != observation.intended_key {
        return Err(discovery_error("restored_key_mismatch"));
    }
    if observation.restored_compatibility != observation.expected_compatibility {
        return Err(discovery_error("restored_compatibility_mismatch"));
    }
    if observation.restored_coverage != observation.expected_coverage {
        return Err(discovery_error("restored_coverage_mismatch"));
    }
    if observation.restored_manifest != observation.intended_manifest {
        return Err(discovery_error("restored_manifest_mismatch"));
    }
    Ok(())
}

pub fn discovery_canonical_bytes(value: &DiscoveryV1) -> Result<Vec<u8>, DiscoveryError> {
    value.validate()?;
    canonical_json_bytes(value).map_err(|error| contract_error(&error))
}

pub fn parse_discovery_bytes(bytes: &[u8]) -> Result<DiscoveryV1, DiscoveryError> {
    let value = parse_strict_json_bytes(bytes, MAX_DISCOVERY_JSON_BYTES)
        .map_err(|error| contract_error(&error))?;
    let record: DiscoveryV1 = serde_json::from_value(value)
        .map_err(|error| discovery_error(&format!("discovery_json:{error}")))?;
    record.validate()?;
    Ok(record)
}

pub fn discovery_digest(record: &DiscoveryV1) -> Result<Digest, DiscoveryError> {
    record.validate()?;
    let bytes = canonical_json_bytes(record).map_err(|error| contract_error(&error))?;
    Ok(digest_b3_typed(&bytes))
}
