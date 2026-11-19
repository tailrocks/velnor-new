// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::SHARED_SOURCES_OWNER;

pub type CacheId = u64;
pub type Timestamp = i64;
pub const DEFAULT_CACHE_BUDGET_BYTES: u64 = 8_000_000_000;
const DAY: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub id: CacheId,
    pub key: String,
    pub reference: String,
    pub size_in_bytes: u64,
    pub last_accessed_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheFamily {
    Target,
    Sources,
    Tools,
    Delta,
    Unknown,
}

#[must_use]
pub fn cache_family(key: &str) -> CacheFamily {
    classify(key).map_or(CacheFamily::Unknown, |(family, _)| family)
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn numeric(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn host(text: &str) -> bool {
    matches!(
        text,
        "Linux-X64" | "Linux-ARM64" | "Windows-X64" | "Windows-ARM64" | "macOS-X64" | "macOS-ARM64"
    )
}

fn classify(key: &str) -> Option<(CacheFamily, &str)> {
    if let Some(rest) = key.strip_prefix("velnor-tools-") {
        let (group, lock) = rest.rsplit_once('-')?;
        let (platform, version) = group.split_once("-mise")?;
        return (hex(lock, 64) && host(platform) && !version.is_empty())
            .then_some((CacheFamily::Tools, platform));
    }
    if let Some(rest) = key.strip_prefix("velnor-src-") {
        let (group, lock) = rest.rsplit_once('-')?;
        let (platform, root) = group.rsplit_once('-')?;
        return (hex(lock, 64) && host(platform) && !root.is_empty())
            .then_some((CacheFamily::Sources, group));
    }
    let (family, rest) = if let Some(rest) = key.strip_prefix("velnor-tgt-") {
        (CacheFamily::Target, rest)
    } else {
        (CacheFamily::Delta, key.strip_prefix("velnor-dlt-")?)
    };
    let (rest, run) = rest.rsplit_once('-')?;
    if !numeric(run) {
        return None;
    }
    let (rest, tail) = rest.rsplit_once('-')?;
    let group = if family == CacheFamily::Target {
        if !hex(tail, 40) {
            return None;
        }
        let (group, lock) = rest.rsplit_once('-')?;
        if !hex(lock, 64) {
            return None;
        }
        group
    } else {
        if !hex(tail, 8) {
            return None;
        }
        rest
    };
    let (prefix, compat) = group.rsplit_once('-')?;
    if family == CacheFamily::Target {
        let (owner, platform) = prefix.rsplit_once('.')?;
        let (job, root) = owner.rsplit_once('.')?;
        return (hex(compat, 16) && host(platform) && !job.is_empty() && !root.is_empty())
            .then_some((family, group));
    }
    let (prefix, arch) = prefix.rsplit_once('-')?;
    let (prefix, os) = prefix.rsplit_once('-')?;
    let (job, root) = prefix.rsplit_once('-')?;
    (hex(compat, 16) && host(&format!("{os}-{arch}")) && !job.is_empty() && !root.is_empty())
        .then_some((family, group))
}

fn retired_sources(key: &str) -> bool {
    classify(key).is_some_and(|(family, group)| {
        family == CacheFamily::Sources
            && !group
                .strip_suffix(SHARED_SOURCES_OWNER)
                .and_then(|platform| platform.strip_suffix('-'))
                .is_some_and(host)
    })
}

fn pull_request(reference: &str) -> Option<u64> {
    let number = reference
        .strip_prefix("refs/pull/")?
        .strip_suffix("/merge")?;
    numeric(number)
        .then(|| number.parse().ok())
        .flatten()
        .filter(|number| *number > 0)
}

fn target_owner(group: &str) -> Option<&str> {
    group.rsplit_once('-').map(|(owner, _)| owner)
}

#[must_use]
pub fn target_family_prefix(prefix: &str) -> Option<&str> {
    let owner = prefix.strip_suffix('-')?;
    let (owner, compat) = owner.rsplit_once('-')?;
    (prefix.starts_with("velnor-tgt-") && hex(compat, 16)).then(|| &prefix[..=owner.len()])
}

#[must_use]
pub fn budget_plan(
    entries: &[CacheEntry],
    open_prs: &BTreeSet<u64>,
    now: Timestamp,
    budget_bytes: u64,
) -> Vec<CacheId> {
    let mut groups: BTreeMap<(CacheFamily, &str, &str), Vec<&CacheEntry>> = BTreeMap::new();
    for entry in entries {
        if let Some((family, group)) = classify(&entry.key)
            && family != CacheFamily::Delta
        {
            groups
                .entry((family, group, &entry.reference))
                .or_default()
                .push(entry);
        }
    }
    let mut ranks = BTreeMap::new();
    let mut current: BTreeMap<(&str, &str), ((Timestamp, CacheId), &str)> = BTreeMap::new();
    for ((family, group, reference), members) in &mut groups {
        members.sort_unstable_by_key(|entry| std::cmp::Reverse((entry.created_at, entry.id)));
        for (rank, entry) in members.iter().enumerate() {
            ranks.insert(entry.id, rank);
        }
        if *family == CacheFamily::Target
            && let (Some(owner), Some(newest)) = (target_owner(group), members.first())
        {
            let stamp = (newest.created_at, newest.id);
            let slot = current.entry((owner, *reference)).or_insert((stamp, group));
            if stamp > slot.0 {
                *slot = (stamp, group);
            }
        }
    }
    let stale_compat = |entry: &CacheEntry, group: &str| {
        target_owner(group)
            .and_then(|owner| current.get(&(owner, entry.reference.as_str())))
            .is_some_and(|(_, newest)| *newest != group)
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let tier = match classify(&entry.key) {
            Some((CacheFamily::Sources, _)) if retired_sources(&entry.key) => 0,
            Some((CacheFamily::Target, group)) if stale_compat(entry, group) => 0,
            Some((CacheFamily::Delta, _)) => {
                let Some(pr) = delta_pull_request(entry) else {
                    continue;
                };
                if !open_prs.contains(&pr) {
                    0
                } else if entry.last_accessed_at <= now.saturating_sub(3 * DAY) {
                    1
                } else {
                    4
                }
            }
            Some((CacheFamily::Sources, _)) if entry.last_accessed_at > now.saturating_sub(DAY) => {
                continue;
            }
            Some((CacheFamily::Target | CacheFamily::Sources, _))
                if ranks.get(&entry.id).is_some_and(|rank| *rank >= 2) =>
            {
                2
            }
            Some((CacheFamily::Tools, _))
                if ranks.get(&entry.id).is_some_and(|rank| *rank > 0)
                    && entry.last_accessed_at <= now.saturating_sub(7 * DAY) =>
            {
                3
            }
            _ => continue,
        };
        candidates.push((
            tier,
            entry.last_accessed_at,
            entry.created_at,
            entry.id,
            entry.size_in_bytes,
        ));
    }
    candidates.sort_unstable();
    let mut total: u128 = entries
        .iter()
        .map(|entry| u128::from(entry.size_in_bytes))
        .sum();
    let mut deleted = Vec::new();
    for (tier, _, _, id, bytes) in candidates {
        if tier != 0 && total <= u128::from(budget_bytes) {
            break;
        }
        deleted.push(id);
        total -= u128::from(bytes);
    }
    deleted
}

#[must_use]
pub fn delta_pull_request(entry: &CacheEntry) -> Option<u64> {
    (cache_family(&entry.key) == CacheFamily::Delta)
        .then(|| pull_request(&entry.reference))
        .flatten()
}

pub(crate) fn configured_budget(value: Option<u64>) -> Result<u64, crate::error::Reason> {
    let value = value.unwrap_or(DEFAULT_CACHE_BUDGET_BYTES);
    if value > DEFAULT_CACHE_BUDGET_BYTES {
        return Err(crate::error::Reason::InvalidValue {
            key: "cache_budget_bytes",
            value: value.to_string(),
        });
    }
    Ok(value)
}
