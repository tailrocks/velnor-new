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

fn require_current(probes: &[SchemaProbe]) -> Result<(), HostError> {
    if probes
        .iter()
        .all(|probe| probe.schema_version == POLICY_SCHEMA)
    {
        Ok(())
    } else {
        Err(HostError::JobAuthorityPolicyOutdated)
    }
}

pub(crate) fn require_current_json_policy(policy_json: &str) -> Result<(), HostError> {
    match serde_json::from_str::<SchemaProbe>(policy_json) {
        Ok(probe) => require_current(&[probe]),
        Err(_) => Ok(()),
    }
}

pub(crate) fn require_current_toml_policies(text: &str) -> Result<(), HostError> {
    match toml::from_str::<HostProbe>(text) {
        Ok(probe) => require_current(&probe.job_authority_policies),
        Err(_) => Ok(()),
    }
}
