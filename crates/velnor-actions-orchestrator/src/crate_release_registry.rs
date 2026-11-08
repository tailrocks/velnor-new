// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod owner;
pub(crate) use owner::linked_github_reviewer;
use owner::read_owners;

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
        let (origin, remote_path) =
            if path.starts_with("/api/v1/crates/") || path.starts_with("/api/v1/users/") {
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
        let validated_path = if let Some(user) = path.strip_prefix("/api/v1/users/") {
            let user = user
                .strip_suffix("?include=linked_accounts")
                .ok_or_else(|| internal("crate_registry_path_invalid"))?;
            if user.is_empty()
                || user.len() > 64
                || !user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err(internal("crate_registry_path_invalid"));
            }
            format!("/api/v1/users/{user}")
        } else {
            path.replace("%2B", "+")
        };
        if !validated_path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'+'))
            || path.contains("..")
        {
            return Err(internal("crate_registry_path_invalid"));
        }
        let timeout =
            velnor_actions_mise::CheckDeadline::cap_to_scope(std::time::Duration::from_secs(30))
                .map_err(|_| internal("crate_registry_deadline"))?;
        let mut response = crate::api::github_evidence_http::agent()?
            .get(format!("{origin}{remote_path}"))
            .header("User-Agent", "velnor-actions crate-release")
            .header("Accept-Encoding", "identity")
            .header("Cache-Control", "no-cache, no-store")
            .config()
            .timeout_global(Some(timeout))
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
    reconcile_registry_archive(
        reader,
        name,
        version,
        expected_owners,
        Some(expected_archive),
        None,
        None,
    )
}

pub(crate) fn revalidate_crate_registry<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
    expected: &CrateRegistryReconciliation,
) -> Result<CrateRegistryReconciliation, OrchestratorError> {
    reconcile_registry_archive(
        reader,
        name,
        version,
        &expected.owners,
        None,
        Some(expected),
        None,
    )
}

pub(super) fn reconcile_historical_crate_registry<R: CrateRegistryRead>(
    reader: &R,
    repository: &str,
    name: &str,
    version: &str,
    expected_owners: &[CrateRegistryOwner],
    expected_archive: &[u8],
    evidence: &crate::crate_release_controller::HistoricalGitHubEvidence,
) -> Result<CrateRegistryReconciliation, OrchestratorError> {
    if !evidence.matches_package(repository, name, version) {
        return Err(internal("crate_registry_historical_identity_mismatch"));
    }
    reconcile_registry_archive(
        reader,
        name,
        version,
        expected_owners,
        Some(expected_archive),
        None,
        Some(evidence),
    )
}

fn reconcile_registry_archive<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
    version: &str,
    expected_owners: &[CrateRegistryOwner],
    expected_archive: Option<&[u8]>,
    frozen: Option<&CrateRegistryReconciliation>,
    historical: Option<&crate::crate_release_controller::HistoricalGitHubEvidence>,
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
    let expected_features = if let Some(expected_archive) = expected_archive {
        crate::release::crate_release_archive::crate_archive_features(
            expected_archive,
            name,
            version,
        )?
    } else {
        frozen
            .ok_or_else(|| internal("crate_registry_checksum_missing"))?
            .features
            .clone()
    };
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
    if let Some(expected_archive) = expected_archive {
        if let Some(evidence) = historical {
            crate::release::crate_release_archive::verify_historical_archive_content(
                expected_archive,
                &archive.body,
                name,
                version,
                checksum,
                evidence.current_sha(),
                evidence.historical_sha(),
            )?;
        } else {
            verify_crate_archive(expected_archive, &archive.body, name, version, checksum)?;
        }
    } else {
        let frozen = frozen.ok_or_else(|| internal("crate_registry_checksum_missing"))?;
        if checksum != frozen.checksum
            || checksum != frozen.downloaded_checksum
            || crate::planner::generator::sha256_hex(&archive.body) != checksum
        {
            return Err(internal("crate_registry_checksum_mismatch"));
        }
    }
    Ok(CrateRegistryReconciliation {
        checksum: checksum.to_owned(),
        downloaded_checksum: crate::planner::generator::sha256_hex(&archive.body),
        owners: expected_owners.to_vec(),
        features: actual_features,
    })
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

#[path = "crate_release_registry_wait.rs"]
mod wait;
pub(crate) use wait::wait_for_crate_index;
pub use wait::wait_for_crate_version;

#[cfg(test)]
#[path = "crate_release_registry_tests.rs"]
mod index_tests;

#[cfg(test)]
#[path = "crate_release_registry_revalidation_http_tests.rs"]
mod revalidation_http_tests;
