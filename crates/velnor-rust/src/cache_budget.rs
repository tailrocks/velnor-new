// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{PRIVATE_SOURCES_TAG, PRIVATE_TARGET_TAG, SHARED_SOURCES_OWNER};

pub type CacheId = u64;
pub type Timestamp = i64;
pub const DEFAULT_CACHE_BUDGET_BYTES: u64 = 8_000_000_000;
const DAY: i64 = 86_400;
const ANCHOR_GENERATIONS: usize = 2;

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
    ToolsAnchor,
    Delta,
    Objects,
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

const KEY_PREFIX: &str = "velnor-";

fn base_tag<'a>(key: &'a str, shared: &str, private: &str) -> Option<(&'a str, bool)> {
    let rest = key.strip_prefix(KEY_PREFIX)?;
    let after = |tag: &str| rest.strip_prefix(tag)?.strip_prefix('-');
    after(shared)
        .map(|rest| (rest, false))
        .or_else(|| after(private).map(|rest| (rest, true)))
}

fn scoped<'a>(key: &'a str, rest: &str, group: &'a str, private: bool) -> &'a str {
    if private {
        &key[KEY_PREFIX.len()..key.len() - rest.len() + group.len()]
    } else {
        group
    }
}

fn job_id(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn anchor_run(run: &str) -> bool {
    run.is_empty()
        || run
            .strip_prefix('-')
            .and_then(|run| run.split_once('-'))
            .is_some_and(|(id, rest)| {
                numeric(id)
                    && (job_id(rest)
                        || rest
                            .split_once('-')
                            .is_some_and(|(attempt, job)| numeric(attempt) && job_id(job)))
            })
}

fn anchor_platform(rest: &str) -> Option<&str> {
    let (platform, rest) = rest.split_once("-mise")?;
    let (version, rest) = rest.split_once('-')?;
    let (lock, run) = rest.split_at_checked(64)?;
    (host(platform)
        && !version.is_empty()
        && hex(lock, 64)
        && !lock.bytes().any(|b| b.is_ascii_uppercase())
        && anchor_run(run))
    .then_some(platform)
}

const OBJECTS_MARK: &str = "-mbx-v1-";

fn toolchain_owner(group: &str) -> Option<&str> {
    group.strip_suffix("-norust").or_else(|| {
        let (owner, tool) = group.rsplit_once("-rust-")?;
        hex(tool, 12).then_some(owner)
    })
}

fn objects_group(key: &str) -> Option<&str> {
    let (host, rest) = key.split_once(OBJECTS_MARK)?;
    let (platform, arch) = host.split_once('-')?;
    let rest = match rest.rsplit_once("-run-") {
        Some((head, run))
            if run
                .split_once('-')
                .is_some_and(|(id, attempt)| numeric(id) && numeric(attempt)) =>
        {
            head
        }
        _ => rest,
    };
    let (body, commit) = rest.rsplit_once('-')?;
    let job = toolchain_owner(body)?.strip_suffix("-dir")?;
    (matches!(platform, "linux" | "darwin" | "win32")
        && matches!(arch, "x64" | "arm64")
        && hex(commit, 40)
        && job_id(job))
    .then(|| &key[..host.len() + OBJECTS_MARK.len() + body.len()])
}

fn classify(key: &str) -> Option<(CacheFamily, &str)> {
    if let Some(group) = objects_group(key) {
        return Some((CacheFamily::Objects, group));
    }
    if let Some(rest) = key.strip_prefix("velnor-tools-anchor-") {
        return anchor_platform(rest).map(|platform| (CacheFamily::ToolsAnchor, platform));
    }
    if let Some(rest) = key.strip_prefix("velnor-tools-") {
        let (group, lock) = rest.rsplit_once('-')?;
        let (platform, version) = group.split_once("-mise")?;
        return (hex(lock, 64) && host(platform) && !version.is_empty())
            .then_some((CacheFamily::Tools, platform));
    }
    if let Some((rest, private)) = base_tag(key, "src", PRIVATE_SOURCES_TAG) {
        let (group, lock) = rest.rsplit_once('-')?;
        let (platform, root) = group.rsplit_once('-')?;
        return (hex(lock, 64) && host(platform) && !root.is_empty())
            .then_some((CacheFamily::Sources, scoped(key, rest, group, private)));
    }
    let (family, whole, private) =
        if let Some((rest, private)) = base_tag(key, "tgt", PRIVATE_TARGET_TAG) {
            (CacheFamily::Target, rest, private)
        } else {
            (CacheFamily::Delta, key.strip_prefix("velnor-dlt-")?, false)
        };
    let (rest, run) = whole.rsplit_once('-')?;
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
        let (pull, group) = rest.split_once('-')?;
        if !pull.strip_prefix("pr").is_some_and(numeric) {
            return None;
        }
        group
    };
    let (prefix, compat) = group.rsplit_once('-')?;
    let (owner, platform) = prefix.rsplit_once('.')?;
    let (job, root) = owner.rsplit_once('.')?;
    (hex(compat, 16) && host(platform) && !job.is_empty() && !root.is_empty())
        .then_some((family, scoped(key, whole, group, private)))
}

fn retired_sources(key: &str) -> bool {
    classify(key).is_some_and(|(family, group)| {
        let group = group
            .strip_prefix(PRIVATE_SOURCES_TAG)
            .and_then(|group| group.strip_prefix('-'))
            .unwrap_or(group);
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

fn owner(family: CacheFamily, group: &str) -> Option<&str> {
    match family {
        CacheFamily::Objects => toolchain_owner(group),
        _ => group.rsplit_once('-').map(|(owner, _)| owner),
    }
}

#[must_use]
pub fn is_sources_prefix(prefix: &str) -> bool {
    base_tag(prefix, "src", PRIVATE_SOURCES_TAG).is_some()
}

#[must_use]
pub fn target_family_prefix(prefix: &str) -> Option<&str> {
    let owner = prefix.strip_suffix('-')?;
    let (owner, compat) = owner.rsplit_once('-')?;
    (base_tag(prefix, "tgt", PRIVATE_TARGET_TAG).is_some() && hex(compat, 16))
        .then(|| &prefix[..=owner.len()])
}

type Groups<'a> = BTreeMap<(CacheFamily, &'a str, &'a str), Vec<&'a CacheEntry>>;
type Newest<'a> = BTreeMap<(&'a str, &'a str), ((Timestamp, CacheId), &'a str)>;

fn ranked<'a>(entries: &'a [CacheEntry]) -> Groups<'a> {
    let mut groups: Groups<'a> = BTreeMap::new();
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
    for members in groups.values_mut() {
        members.sort_unstable_by_key(|entry| std::cmp::Reverse((entry.created_at, entry.id)));
    }
    groups
}

fn newest_generations<'a>(groups: &Groups<'a>) -> Newest<'a> {
    let mut newest: Newest<'a> = BTreeMap::new();
    for ((family, group, reference), members) in groups {
        if matches!(family, CacheFamily::Target | CacheFamily::Objects)
            && let (Some(owner), Some(first)) = (owner(*family, group), members.first())
        {
            let stamp = (first.created_at, first.id);
            let slot = newest.entry((owner, reference)).or_insert((stamp, group));
            if stamp > slot.0 {
                *slot = (stamp, group);
            }
        }
    }
    newest
}

fn tier(
    entry: &CacheEntry,
    rank: usize,
    newest: &Newest<'_>,
    open_prs: &BTreeSet<u64>,
    now: Timestamp,
) -> Option<u8> {
    let idle = |days: i64| entry.last_accessed_at <= now.saturating_sub(days * DAY);
    match classify(&entry.key)? {
        (CacheFamily::Sources, _) if retired_sources(&entry.key) => Some(0),
        (family @ (CacheFamily::Target | CacheFamily::Objects), group)
            if owner(family, group)
                .and_then(|owner| newest.get(&(owner, entry.reference.as_str())))
                .is_some_and(|(_, current)| *current != group) =>
        {
            Some(0)
        }
        (CacheFamily::Objects, _)
            if pull_request(&entry.reference).is_some_and(|pr| !open_prs.contains(&pr)) =>
        {
            Some(0)
        }
        (CacheFamily::Delta, _) => {
            let pr = scoped_pull_request(entry)?;
            Some(if !open_prs.contains(&pr) {
                0
            } else if idle(3) {
                1
            } else {
                4
            })
        }
        (CacheFamily::Sources | CacheFamily::ToolsAnchor, _) if !idle(1) => None,
        (CacheFamily::Target | CacheFamily::Sources | CacheFamily::Objects, _) if rank >= 2 => {
            Some(2)
        }
        (CacheFamily::ToolsAnchor, _) if rank >= ANCHOR_GENERATIONS => Some(0),
        (CacheFamily::Tools, _) if rank > 0 && idle(7) => Some(3),
        _ => None,
    }
}

#[must_use]
pub fn budget_plan(
    entries: &[CacheEntry],
    open_prs: &BTreeSet<u64>,
    now: Timestamp,
    budget_bytes: u64,
) -> Vec<CacheId> {
    let groups = ranked(entries);
    let newest = newest_generations(&groups);
    let ranks: BTreeMap<CacheId, usize> = groups
        .values()
        .flat_map(|members| members.iter().enumerate())
        .map(|(rank, entry)| (entry.id, rank))
        .collect();
    let mut candidates: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let rank = ranks.get(&entry.id).copied().unwrap_or(0);
            tier(entry, rank, &newest, open_prs, now).map(|tier| {
                (
                    tier,
                    entry.last_accessed_at,
                    entry.created_at,
                    entry.id,
                    entry.size_in_bytes,
                )
            })
        })
        .collect();
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
pub fn scoped_pull_request(entry: &CacheEntry) -> Option<u64> {
    matches!(
        cache_family(&entry.key),
        CacheFamily::Delta | CacheFamily::Objects
    )
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
