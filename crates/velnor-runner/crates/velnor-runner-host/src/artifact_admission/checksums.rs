//! Exact four-row SHA256SUMS parser for the immutable image release.

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

#[cfg(test)]
mod tests {
    use super::{ARCHIVE_ASSET, MANIFEST_ASSET, asset_digest, parse};
    use crate::error::HostError;

    fn checksums() -> String {
        let digest = "11".repeat(32);
        format!(
            "{digest}  velnor-runner-linux-amd64.tar\n{digest}  velnor-dind-linux-amd64.tar\n{digest}  {ARCHIVE_ASSET}\n{digest}  {MANIFEST_ASSET}\n"
        )
    }

    #[test]
    fn accepts_exact_asset_set() -> Result<(), HostError> {
        let bytes = checksums();
        let parsed = parse(bytes.as_bytes())?;
        if asset_digest(&parsed, ARCHIVE_ASSET)? != "11".repeat(32) {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    #[test]
    fn rejects_duplicate_unknown_uppercase_and_missing_assets() {
        let base = checksums();
        let duplicate = format!("{base}{}  {}\n", "22".repeat(32), ARCHIVE_ASSET);
        assert_eq!(parse(duplicate.as_bytes()), Err(HostError::Identity));
        let unknown = format!("{}{}  unknown.tar\n", base, "22".repeat(32));
        assert_eq!(parse(unknown.as_bytes()), Err(HostError::Identity));
        let uppercase = base.replace(&"11".repeat(32), &"AA".repeat(32));
        assert_eq!(parse(uppercase.as_bytes()), Err(HostError::Identity));
        let manifest_row = format!("{}  {MANIFEST_ASSET}\n", "11".repeat(32));
        let missing = base.replace(&manifest_row, "");
        assert_eq!(parse(missing.as_bytes()), Err(HostError::Identity));
        let self_row = format!("{base}{}  SHA256SUMS\n", "22".repeat(32));
        assert_eq!(parse(self_row.as_bytes()), Err(HostError::Identity));
    }
}
