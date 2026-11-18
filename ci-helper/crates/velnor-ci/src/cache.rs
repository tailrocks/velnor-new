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
pub fn stale_entries(entries: &[CacheEntry], prefix: &str, keep: usize) -> Vec<u64> {
    let mut family: Vec<(i64, u64)> = entries
        .iter()
        .filter(|entry| entry.key.starts_with(prefix))
        .map(|entry| (parse_utc(&entry.created_at).unwrap_or(0), entry.id))
        .collect();
    family.sort_unstable_by(|a, b| b.cmp(a));
    family.into_iter().skip(keep).map(|(_, id)| id).collect()
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
