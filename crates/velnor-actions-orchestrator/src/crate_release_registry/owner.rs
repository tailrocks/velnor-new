// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateRegistryOwner, CrateRegistryRead, OrchestratorError, Value, internal};

pub(crate) fn linked_github_reviewer<R: CrateRegistryRead>(
    reader: &R,
    owner: &CrateRegistryOwner,
    reviewer_id: u64,
) -> Result<bool, OrchestratorError> {
    if owner.kind != "user"
        || owner.login.is_empty()
        || owner.login.len() > 64
        || !owner
            .login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(internal("crate_registry_owner_invalid"));
    }
    let response = reader.get(&format!(
        "/api/v1/users/{}?include=linked_accounts",
        owner.login
    ))?;
    if response.status != 200 {
        return Err(internal("crate_registry_owner_unavailable"));
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&response.body, 1024 * 1024)
            .map_err(|_| internal("crate_registry_owner_invalid"))?;
    if value["user"]["id"].as_u64() != Some(owner.id)
        || value["user"]["login"].as_str() != Some(owner.login.as_str())
    {
        return Err(internal("crate_registry_owner_invalid"));
    }
    let accounts = value["linked_accounts"]
        .as_array()
        .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
    if accounts.is_empty() || accounts.len() > 256 {
        return Err(internal("crate_registry_owner_invalid"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for account in accounts {
        if account["provider"].as_str() != Some("github") {
            return Err(internal("crate_registry_owner_invalid"));
        }
        let raw = account["account_id"]
            .as_str()
            .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
        let id = raw
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0 && id.to_string() == raw)
            .ok_or_else(|| internal("crate_registry_owner_invalid"))?;
        if !ids.insert(id) {
            return Err(internal("crate_registry_owner_invalid"));
        }
    }
    Ok(ids.contains(&reviewer_id))
}

pub(super) fn read_owners<R: CrateRegistryRead>(
    reader: &R,
    name: &str,
) -> Result<Vec<CrateRegistryOwner>, OrchestratorError> {
    let response = reader.get(&format!("/api/v1/crates/{name}/owners"))?;
    if response.status != 200 {
        return Err(internal("crate_registry_owners_unavailable"));
    }
    let body: Value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&response.body, 1024 * 1024)
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
    let identities: std::collections::BTreeSet<_> = owners
        .iter()
        .map(|owner| (owner.id, owner.kind.as_str()))
        .collect();
    if owners.is_empty() || identities.len() != owners.len() {
        return Err(internal("crate_registry_owners_invalid"));
    }
    Ok(owners)
}
