// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub(super) resource_kind: String,
    pub(super) role: String,
    pub(super) docker_name: String,
    pub(super) docker_id: String,
    pub(super) reservation_id: i64,
    pub(super) connection_id: String,
    pub(super) operation_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    driver: Option<String>,
    labels: BTreeMap<String, String>,
}

pub(super) fn validate(json: &str, digest: &str) -> Result<Identity, HostError> {
    if json.len() > 16384
        || crate::digest_hex::lowercase_hex(&Sha256::digest(json.as_bytes())) != digest
    {
        return Err(HostError::Journal);
    }
    let identity: Identity = serde_json::from_str(json).map_err(|_| HostError::Journal)?;
    if serde_json::to_string(&identity).map_err(|_| HostError::Journal)? != json {
        return Err(HostError::Journal);
    }
    let expected = expected_labels(&identity)?;
    if expected
        .iter()
        .any(|(key, value)| identity.labels.get(*key) != Some(value))
        || identity.labels.len() != expected.len()
    {
        return Err(HostError::Journal);
    }
    Ok(identity)
}

fn expected_labels(identity: &Identity) -> Result<BTreeMap<&'static str, String>, HostError> {
    let connection = identity.connection_id.clone();
    let reservation = identity.reservation_id.to_string();
    let operation = identity.operation_id.to_string();
    match identity.resource_kind.as_str() {
        "container" if identity.driver.is_none() => Ok(BTreeMap::from([
            ("io.velnor.managed", "true".to_owned()),
            ("io.velnor.connection-id", connection),
            ("io.velnor.reservation-id", reservation),
            ("io.velnor.operation-id", operation),
            ("io.velnor.role", identity.role.clone()),
        ])),
        "volume"
            if identity.driver.as_deref() == Some("local")
                && identity.docker_id == identity.docker_name =>
        {
            Ok(BTreeMap::from([
                ("velnor.connection", connection),
                ("velnor.reservation", reservation),
                ("velnor.operation", operation),
                ("velnor.role", identity.role.clone()),
            ]))
        }
        "network" if identity.driver.is_none() && identity.role == "private" => {
            Ok(BTreeMap::from([
                ("io.velnor.managed", "true".to_owned()),
                ("io.velnor.connection-id", connection),
                ("io.velnor.reservation-id", reservation),
                ("io.velnor.operation-id", operation),
                ("io.velnor.engine-id", identity.engine_id.clone()),
            ]))
        }
        _ => Err(HostError::Journal),
    }
}
