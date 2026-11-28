// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use super::POLICY_SCHEMA;
use crate::error::HostError;

#[derive(Deserialize)]
struct SchemaProbe {
    schema_version: u32,
}

#[derive(Deserialize)]
struct HostProbe {
    #[serde(default)]
    job_authority_policies: Vec<SchemaProbe>,
}

fn outdated(probes: &[SchemaProbe]) -> Option<u32> {
    probes
        .iter()
        .map(|probe| probe.schema_version)
        .find(|version| *version != POLICY_SCHEMA)
}

fn outdated_json_schema(policy_json: &[u8]) -> Option<u32> {
    serde_json::from_slice::<SchemaProbe>(policy_json)
        .ok()
        .and_then(|probe| outdated(&[probe]))
}

pub(crate) fn require_current_json_policy(policy_json: &[u8]) -> Result<(), HostError> {
    match outdated_json_schema(policy_json) {
        Some(found) => Err(HostError::JobAuthorityPolicySchema {
            found,
            expected: POLICY_SCHEMA,
        }),
        None => Ok(()),
    }
}

pub(crate) fn require_current_journal_policy(policy_json: &[u8]) -> Result<(), HostError> {
    match outdated_json_schema(policy_json) {
        Some(found) => Err(HostError::JournalPolicySchema {
            found,
            expected: POLICY_SCHEMA,
        }),
        None => Ok(()),
    }
}

pub(crate) fn require_current_toml_policies(text: &str) -> Result<(), HostError> {
    match toml::from_str::<HostProbe>(text)
        .ok()
        .and_then(|probe| outdated(&probe.job_authority_policies))
    {
        Some(found) => Err(HostError::JobAuthorityPolicySchema {
            found,
            expected: POLICY_SCHEMA,
        }),
        None => Ok(()),
    }
}
