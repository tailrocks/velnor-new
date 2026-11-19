// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
pub use crate::io::cache_api::trim;
use crate::logscan::LogScan;
use crate::time::parse_utc;

const SECONDS_PER_DAY: i64 = 86_400;

#[must_use]
pub fn should_save(scan: &LogScan) -> bool {
    scan.crates_compiled() > 0
}

#[must_use]
pub fn born_expired(born: &str, now_seconds: i64, max_age_days: u64) -> bool {
    let Some(born_seconds) = parse_utc(&format!("{}T00:00:00Z", born.trim())) else {
        return true;
    };
    let max = i64::try_from(max_age_days).unwrap_or(i64::MAX / SECONDS_PER_DAY);
    now_seconds - born_seconds > max.saturating_mul(SECONDS_PER_DAY)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CacheEntry {
    pub id: u64,
    pub key: String,
    pub created_at: String,
    #[serde(default)]
    pub last_accessed_at: Option<String>,
}

#[derive(Deserialize)]
struct Listing {
    actions_caches: Vec<CacheEntry>,
}

pub fn parse_entries(json: &str) -> Result<Vec<CacheEntry>, CiError> {
    serde_json::from_str::<Listing>(json)
        .map(|listing| listing.actions_caches)
        .map_err(|e| CiError::new(format!("the cache listing is not readable: {e}")))
}

#[must_use]
pub fn stale_entries(
    entries: &[CacheEntry],
    prefix: &str,
    keep: usize,
    in_use_since: Option<i64>,
) -> Vec<u64> {
    let mut family: Vec<(i64, u64, Option<i64>)> = entries
        .iter()
        .filter(|entry| entry.key.starts_with(prefix))
        .map(|entry| {
            (
                parse_utc(&entry.created_at).unwrap_or(0),
                entry.id,
                entry.last_accessed_at.as_deref().and_then(parse_utc),
            )
        })
        .collect();
    family.sort_unstable_by(|a, b| b.cmp(a));
    family
        .into_iter()
        .skip(keep)
        .filter(|(_, _, accessed)| {
            !in_use_since.is_some_and(|since| accessed.is_some_and(|at| at > since))
        })
        .map(|(_, id, _)| id)
        .collect()
}

#[must_use]
pub fn foreign_entries(entries: &[CacheEntry], prefix: &str) -> Vec<u64> {
    let Some(family) = velnor_rust::target_family_prefix(prefix) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| entry.key.starts_with(family) && !entry.key.starts_with(prefix))
        .map(|entry| entry.id)
        .collect()
}

pub const KEEP_NEWEST: usize = 2;
pub const MAX_PAGES: usize = 20;

pub(crate) fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
