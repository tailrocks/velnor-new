// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateRegistryRead, OrchestratorError, internal, validate_name_version};
use serde::Deserialize;

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
