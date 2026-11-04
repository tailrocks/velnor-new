// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal, verify_crate_archive};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateRegistryOwner {
    pub id: u64,
    pub login: String,
    pub kind: String,
}

#[derive(Debug)]
pub struct CrateRegistryResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct NativeCrateRegistryRead;

impl CrateRegistryRead for NativeCrateRegistryRead {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        use std::io::Read as _;
        let (origin, remote_path) = if path.starts_with("/api/v1/crates/") {
            ("https://crates.io", path)
        } else if path.starts_with("/crates/") {
            ("https://static.crates.io", path)
        } else if path.starts_with("/index/") {
            ("https://index.crates.io", &path[6..])
        } else {
            return Err(internal("crate_registry_path_invalid"));
        };
        #[cfg(test)]
        let test_origin =
            crate::crate_release_http::test_loopback_origin("VELNOR_TEST_REGISTRY_ORIGIN")?;
        #[cfg(test)]
        let origin = test_origin.as_deref().unwrap_or(origin);
        let validated_path = path.replace("%2B", "+");
        if !validated_path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'+'))
            || path.contains("..")
        {
            return Err(internal("crate_registry_path_invalid"));
        }
        let mut response = crate::api::github_evidence_http::agent()?
            .get(format!("{origin}{remote_path}"))
            .header("User-Agent", "velnor-actions crate-release")
            .header("Accept-Encoding", "identity")
            .header("Cache-Control", "no-cache, no-store")
            .config()
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .call()
            .map_err(|_| internal("crate_registry_transport"))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut body)
            .map_err(|_| internal("crate_registry_read"))?;
        if body.len() > 64 * 1024 * 1024 {
            return Err(internal("crate_registry_body_limit"));
        }
        Ok(CrateRegistryResponse { status, body })
    }
}

pub trait CrateRegistryRead {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateRegistryPresence {
    Absent,
    Present,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CrateRegistryOwnership {
    NewCrate,
    Existing { owners: Vec<CrateRegistryOwner> },
}

pub fn inspect_crate_registry<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
) -> Result<(CrateRegistryOwnership, CrateRegistryPresence), OrchestratorError> {
    validate_name_version(name, version)?;
    let root = reader.get(&format!("/api/v1/crates/{name}"))?;
    if root.status == 404 {
        let owners = reader.get(&format!("/api/v1/crates/{name}/owners"))?;
        let detail = reader.get(&format!(
            "/api/v1/crates/{name}/{}",
            version.replace('+', "%2B")
        ))?;
        if owners.status != 404 || detail.status != 404 {
            return Err(internal("crate_registry_new_inconsistent"));
        }
        return Ok((
            CrateRegistryOwnership::NewCrate,
            CrateRegistryPresence::Absent,
        ));
    }
    if root.status != 200 {
        return Err(internal("crate_registry_versions_unavailable"));
    }
    let (owners, presence) = inspect_existing_registry(reader, name, version)?;
    Ok((CrateRegistryOwnership::Existing { owners }, presence))
}

fn inspect_existing_registry<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
) -> Result<(Vec<CrateRegistryOwner>, CrateRegistryPresence), OrchestratorError> {
    validate_name_version(name, version)?;
    let owners = read_owners(reader, name)?;
    let inventory = crate::crate_registry_inventory::inspect_crate_versions(reader, name)
        .map_err(|_| internal("crate_registry_versions_unavailable"))?;
    if inventory.has_build_collision(version) {
        return Err(internal("crate_registry_version_collision"));
    }
    let response = reader.get(&format!(
        "/api/v1/crates/{name}/{}",
        version.replace('+', "%2B")
    ))?;
    let presence = match response.status {
        404 => CrateRegistryPresence::Absent,
        200 => CrateRegistryPresence::Present,
        _ => return Err(internal("crate_registry_version_unavailable")),
    };
    if (presence == CrateRegistryPresence::Present) != inventory.versions.contains_key(version) {
        return Err(internal("crate_registry_versions_inconsistent"));
    }
    if presence == CrateRegistryPresence::Present {
        let detail: Value = serde_json::from_slice(&response.body)
            .map_err(|_| internal("crate_registry_response_invalid"))?;
        let record = inventory
            .versions
            .get(version)
            .ok_or_else(|| internal("crate_registry_versions_inconsistent"))?;
        let item = &detail["version"];
        if item["id"].as_u64() != Some(record.id)
            || item["crate"].as_str() != Some(name)
            || item["num"].as_str() != Some(version)
            || item["yanked"].as_bool() != Some(record.yanked)
        {
            return Err(internal("crate_registry_identity_invalid"));
        }
    }
    Ok((owners, presence))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateRegistryReconciliation {
    pub checksum: String,
    pub downloaded_checksum: String,
    pub owners: Vec<CrateRegistryOwner>,
    pub features: std::collections::BTreeMap<String, Vec<String>>,
}

pub fn reconcile_crate_registry<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
    expected_owners: &[CrateRegistryOwner],
    expected_archive: &[u8],
) -> Result<CrateRegistryReconciliation, OrchestratorError> {
    validate_name_version(name, version)?;
    if expected_owners.is_empty() || read_owners(reader, name)? != expected_owners {
        return Err(internal("crate_registry_owners_mismatch"));
    }
    let response = reader.get(&format!(
        "/api/v1/crates/{name}/{}",
        version.replace('+', "%2B")
    ))?;
    if response.status != 200 {
        return Err(internal("crate_registry_version_unavailable"));
    }
    let body: Value = serde_json::from_slice(&response.body)
        .map_err(|_| internal("crate_registry_response_invalid"))?;
    let item = &body["version"];
    if item["crate"].as_str() != Some(name)
        || item["num"].as_str() != Some(version)
        || item["yanked"].as_bool() != Some(false)
    {
        return Err(internal("crate_registry_identity_invalid"));
    }
    let expected_features =
        crate::crate_release_archive::crate_archive_features(expected_archive, name, version)?;
    let mut actual_features: std::collections::BTreeMap<String, Vec<String>> =
        serde_json::from_value(item["features"].clone())
            .map_err(|_| internal("crate_registry_features_invalid"))?;
    for members in actual_features.values_mut() {
        members.sort();
    }
    if actual_features != expected_features {
        return Err(internal("crate_registry_features_mismatch"));
    }
    let checksum = item["checksum"]
        .as_str()
        .ok_or_else(|| internal("crate_registry_checksum_missing"))?;
    let archive = reader.get(&format!(
        "/crates/{name}/{name}-{}.crate",
        version.replace('+', "%2B")
    ))?;
    if archive.status != 200 {
        return Err(internal("crate_registry_archive_unavailable"));
    }
    verify_crate_archive(expected_archive, &archive.body, name, version, checksum)?;
    Ok(CrateRegistryReconciliation {
        checksum: checksum.to_owned(),
        downloaded_checksum: crate::generator::sha256_hex(&archive.body),
        owners: expected_owners.to_vec(),
        features: actual_features,
    })
}

fn read_owners<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
) -> Result<Vec<CrateRegistryOwner>, OrchestratorError> {
    let response = reader.get(&format!("/api/v1/crates/{name}/owners"))?;
    if response.status != 200 {
        return Err(internal("crate_registry_owners_unavailable"));
    }
    let body: Value = serde_json::from_slice(&response.body)
        .map_err(|_| internal("crate_registry_owners_invalid"))?;
    let users = body["users"]
        .as_array()
        .ok_or_else(|| internal("crate_registry_owners_invalid"))?;
    let mut owners = Vec::with_capacity(users.len());
    for user in users {
        let id = user["id"]
            .as_u64()
            .filter(|id| *id != 0)
            .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
        let login = user["login"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
        let kind = user["kind"]
            .as_str()
            .filter(|kind| matches!(*kind, "user" | "team"))
            .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
        owners.push(CrateRegistryOwner {
            id,
            login: login.to_owned(),
            kind: kind.to_owned(),
        });
    }
    owners.sort();
    if owners.is_empty() || owners.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(internal("crate_registry_owners_invalid"));
    }
    Ok(owners)
}

fn validate_name_version(name: &str, version: &str) -> Result<(), OrchestratorError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || version.len() > 150
        || !velnor_actions_contract::config::is_cargo_version(version)
    {
        return Err(internal("crate_registry_request_invalid"));
    }
    Ok(())
}

pub(crate) fn wait_for_crate_index<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
    checksum: &str,
    attempts: usize,
    mut wait: impl FnMut(),
) -> Result<(), OrchestratorError> {
    validate_name_version(name, version)?;
    if attempts == 0
        || checksum.len() != 64
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(internal("crate_index_invalid"));
    }
    let name = name.to_ascii_lowercase();
    let prefix = match name.len() {
        1 => "1".to_owned(),
        2 => "2".to_owned(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    };
    let path = format!("/index/{prefix}/{name}");
    for attempt in 0..attempts {
        let response = reader.get(&path)?;
        let found = match response.status {
            404 => false,
            200 => sparse_entry_matches(&response.body, &name, version, checksum)?,
            _ => return Err(internal("crate_index_unavailable")),
        };
        if found {
            return Ok(());
        }
        if attempt + 1 == attempts {
            return Err(internal("crate_index_not_visible"));
        }
        wait();
    }
    Err(internal("crate_index_not_visible"))
}

#[derive(Deserialize)]
struct SparseIndexEntry {
    name: String,
    vers: String,
    cksum: String,
    yanked: bool,
}

fn sparse_entry_matches(
    body: &[u8],
    name: &str,
    version: &str,
    checksum: &str,
) -> Result<bool, OrchestratorError> {
    if body.len() > 8 * 1024 * 1024 {
        return Err(internal("crate_index_invalid"));
    }
    let text = std::str::from_utf8(body).map_err(|_| internal("crate_index_invalid"))?;
    let mut found = false;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let entry: SparseIndexEntry =
            serde_json::from_str(line).map_err(|_| internal("crate_index_invalid"))?;
        if !entry.name.eq_ignore_ascii_case(name) {
            return Err(internal("crate_index_invalid"));
        }
        if entry.vers == version {
            if found || entry.cksum != checksum || entry.yanked {
                return Err(internal("crate_index_mismatch"));
            }
            found = true;
        }
    }
    Ok(found)
}

pub fn wait_for_crate_version<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
    attempts: usize,
    mut wait: impl FnMut(),
) -> Result<(), OrchestratorError> {
    validate_name_version(name, version)?;
    if attempts == 0 {
        return Err(internal("crate_registry_attempts_invalid"));
    }
    for attempt in 0..attempts {
        let response = reader.get(&format!(
            "/api/v1/crates/{name}/{}",
            version.replace('+', "%2B")
        ))?;
        match response.status {
            200 => return Ok(()),
            404 if attempt + 1 < attempts => wait(),
            404 => return Err(internal("crate_registry_version_not_visible")),
            _ => return Err(internal("crate_registry_version_unavailable")),
        }
    }
    Err(internal("crate_registry_version_not_visible"))
}

#[cfg(test)]
#[path = "crate_release_registry_tests.rs"]
mod index_tests;
