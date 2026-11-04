// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_registry::CrateRegistryRead;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateRegistryVersion {
    pub id: u64,
    pub yanked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateRegistryInventory {
    pub registry: String,
    pub package: String,
    pub response_sha256: String,
    pub versions: BTreeMap<String, CrateRegistryVersion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateInventoryError {
    InvalidName,
    NotFound,
    Forbidden,
    RateLimited,
    ServerError,
    UnexpectedStatus,
    Transport,
    InvalidResponse,
}

impl CrateRegistryInventory {
    #[must_use]
    pub fn snapshot_sha256(&self) -> String {
        let mut canonical = format!(
            "registry-snapshot-v1\n{}:{}\n{}:{}\n",
            self.registry.len(),
            self.registry,
            self.package.len(),
            self.package
        );
        for (version, entry) in &self.versions {
            canonical.push_str(&version.len().to_string());
            canonical.push(':');
            canonical.push_str(version);
            canonical.push(':');
            canonical.push_str(&entry.id.to_string());
            canonical.push(':');
            canonical.push(if entry.yanked { '1' } else { '0' });
            canonical.push('\n');
        }
        crate::generator::sha256_hex(canonical.as_bytes())
    }
    #[must_use]
    pub fn available_versions(&self) -> BTreeSet<String> {
        self.versions
            .iter()
            .filter(|(_, entry)| !entry.yanked)
            .map(|(version, _)| version.clone())
            .collect()
    }
    #[must_use]
    pub fn has_build_collision(&self, version: &str) -> bool {
        self.versions.keys().any(|existing| {
            existing != version && existing.split('+').next() == version.split('+').next()
        })
    }
}

#[derive(Deserialize)]
struct RegistryReply {
    #[serde(rename = "crate")]
    package: RegistryReplyCrate,
    versions: Vec<RegistryReplyVersion>,
}
#[derive(Deserialize)]
struct RegistryReplyCrate {
    id: String,
    name: String,
    versions: Vec<u64>,
}
#[derive(Deserialize)]
struct RegistryReplyVersion {
    id: u64,
    #[serde(rename = "crate")]
    package: String,
    num: String,
    yanked: bool,
}

pub fn inspect_crate_versions<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
) -> Result<CrateRegistryInventory, CrateInventoryError> {
    use CrateInventoryError as E;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(E::InvalidName);
    }
    let response = reader
        .get(&format!("/api/v1/crates/{name}"))
        .map_err(|_| E::Transport)?;
    match response.status {
        200 => {}
        404 => return Err(E::NotFound),
        403 => return Err(E::Forbidden),
        429 => return Err(E::RateLimited),
        500..=599 => return Err(E::ServerError),
        _ => return Err(E::UnexpectedStatus),
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(E::InvalidResponse);
    }
    let body: RegistryReply =
        serde_json::from_slice(&response.body).map_err(|_| E::InvalidResponse)?;
    if body.package.id != name || body.package.name != name {
        return Err(E::InvalidResponse);
    }
    let mut expected = BTreeSet::new();
    for id in body.package.versions {
        if id == 0 || !expected.insert(id) {
            return Err(E::InvalidResponse);
        }
    }
    let mut actual = BTreeSet::new();
    let mut versions = BTreeMap::new();
    for item in body.versions {
        if item.id == 0
            || item.package != name
            || !velnor_actions_contract::config::is_cargo_version(&item.num)
            || !actual.insert(item.id)
            || versions
                .insert(
                    item.num,
                    CrateRegistryVersion {
                        id: item.id,
                        yanked: item.yanked,
                    },
                )
                .is_some()
        {
            return Err(E::InvalidResponse);
        }
    }
    if expected.is_empty() || expected != actual {
        return Err(E::InvalidResponse);
    }
    Ok(CrateRegistryInventory {
        registry: "crates-io".into(),
        package: name.into(),
        response_sha256: crate::generator::sha256_hex(&response.body),
        versions,
    })
}
