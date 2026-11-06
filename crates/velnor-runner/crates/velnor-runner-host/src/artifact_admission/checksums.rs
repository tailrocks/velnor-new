// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::HostError;

use super::hash::is_lower_hex;
use super::manifest::{ARCHIVE_ASSET, MANIFEST_ASSET};

pub(super) const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
const EXPECTED_ASSETS: [&str; 4] = [
    "velnor-runner-linux-amd64.tar",
    "velnor-dind-linux-amd64.tar",
    ARCHIVE_ASSET,
    MANIFEST_ASSET,
];

pub(super) fn parse(bytes: &[u8]) -> Result<BTreeMap<String, String>, HostError> {
    if bytes.is_empty() || bytes.len() > MAX_CHECKSUM_BYTES || !bytes.ends_with(b"\n") {
        return Err(HostError::Frame);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?;
    let mut values = BTreeMap::new();
    for line in text
        .strip_suffix('\n')
        .ok_or(HostError::Identity)?
        .split('\n')
    {
        let (digest, name) = line.split_once("  ").ok_or(HostError::Identity)?;
        if !is_lower_hex(digest, 64)
            || name.is_empty()
            || name.chars().any(char::is_whitespace)
            || !EXPECTED_ASSETS.contains(&name)
            || values.insert(name.to_owned(), digest.to_owned()).is_some()
        {
            return Err(HostError::Identity);
        }
    }
    if values.len() != EXPECTED_ASSETS.len()
        || EXPECTED_ASSETS
            .iter()
            .any(|name| !values.contains_key(*name))
    {
        return Err(HostError::Identity);
    }
    Ok(values)
}

pub(super) fn asset_digest<'a>(
    checksums: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, HostError> {
    checksums
        .get(name)
        .map(String::as_str)
        .ok_or(HostError::Identity)
}
