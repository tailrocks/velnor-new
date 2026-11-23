// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::report::{Hit, LaneMetrics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const TOOLCHAIN_INSTALLS: &str = "velnor-toolchain-installs.txt";

pub const SEED_TRUNCATED: &str = "velnor-mbx-seed-truncated";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MbxCompilerWork {
    pub invocations: u64,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxSession {
    pub hits: u64,
    pub misses: u64,
    pub not_looked_up: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bypassed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifications: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bypasses: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub compiler: BTreeMap<String, MbxCompilerWork>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub probe_only: bool,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxEvidence {
    #[serde(default)]
    pub declared: bool,
    pub store_primed: bool,
    #[serde(default)]
    pub seed_truncated: bool,
    pub sessions: Vec<MbxSession>,
    #[serde(default)]
    pub unparsed: u64,
}

impl MbxEvidence {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.declared
            && !self.store_primed
            && !self.seed_truncated
            && self.sessions.is_empty()
            && self.unparsed == 0
    }
}

#[must_use]
pub fn seed_truncated(runner_temp: &Path) -> bool {
    runner_temp.join(SEED_TRUNCATED).is_file()
}

const OBJECT_CACHE_LABEL: &str = "object cache:";

fn cache_text(line: &str) -> Option<(bool, &str)> {
    let text = line.split_once("mbx[cache]: ")?.1;
    Some(match text.strip_prefix(OBJECT_CACHE_LABEL) {
        Some(rest) => (true, rest.trim_start()),
        None if !text.trim_end().is_empty() && OBJECT_CACHE_LABEL.starts_with(text.trim_end()) => {
            (true, "")
        }
        None => (false, text),
    })
}

fn has_transfer_suffix(tail: &str) -> bool {
    let clause = tail.split(',').next().unwrap_or_default();
    matches!(
        clause.split_whitespace().collect::<Vec<_>>().as_slice(),
        [size, unit, "downloaded"]
            if size.starts_with(|character: char| character.is_ascii_digit())
                && unit.chars().all(|character| character.is_ascii_alphabetic())
    )
}

fn looks_like_lookup_summary(text: &str) -> bool {
    let counter = |word: &str| matches!(word, "hit" | "hits" | "miss" | "misses");
    text.split(|character: char| !character.is_ascii_alphabetic())
        .any(counter)
        || ["downloaded", "uploaded", "stored locally"]
            .iter()
            .any(|tail| text.contains(tail))
}

fn parse_lookup_summary(summary: &str) -> Option<MbxSession> {
    let (counts, tail) = summary.split_once(';')?;
    if !has_transfer_suffix(tail) {
        return None;
    }
    let mut session = MbxSession::default();
    let mut seen: Vec<&str> = Vec::new();
    for clause in counts.split(',') {
        let (count, name) = clause.trim().split_once(' ')?;
        let count: u64 = count.parse().ok()?;
        let name = match name.trim() {
            "hit" | "hits" => "hits",
            "miss" | "misses" => "misses",
            "not looked up" => "not looked up",
            "bypassed" => "bypassed",
            "incremental" => "incremental",
            _ => return None,
        };
        if seen.contains(&name) {
            return None;
        }
        seen.push(name);
        match name {
            "hits" => session.hits = count,
            "misses" => session.misses = count,
            "not looked up" | "incremental" => {
                session.not_looked_up = session.not_looked_up.saturating_add(count);
            }
            _ => {}
        }
    }
    (!seen.is_empty()).then_some(session)
}

#[must_use]
pub fn mbx_sessions(log: &[u8]) -> Vec<MbxSession> {
    String::from_utf8_lossy(log)
        .lines()
        .filter_map(|line| parse_lookup_summary(cache_text(line)?.1))
        .collect()
}

#[must_use]
pub fn mbx_unparsed_lines(log: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(log)
        .lines()
        .filter(|line| {
            cache_text(line).is_some_and(|(labelled, text)| {
                (labelled || looks_like_lookup_summary(text))
                    && parse_lookup_summary(text).is_none()
            })
        })
        .map(|line| line.trim().to_owned())
        .collect()
}

fn compiled_registry_unit(metrics: &LaneMetrics) -> bool {
    metrics.invocations.iter().any(|invocation| {
        invocation.artifacts.iter().any(|artifact| {
            !artifact.fresh
                && crate::cargo_package::PackageIdentity::parse(&artifact.package_id).is_some_and(
                    |package| {
                        package
                            .source
                            .is_some_and(|source| source.starts_with("registry+"))
                    },
                )
        })
    })
}

#[must_use]
pub fn waste(metrics: &LaneMetrics) -> Vec<String> {
    let mut violations = Vec::new();
    for cache in &metrics.caches {
        if cache.family == "sources"
            && cache.hit == Hit::Miss
            && cache.entry_before_job == Some(true)
        {
            violations.push(format!(
                "sources cache missed {} although a saved entry existed before the job started",
                cache.requested_key
            ));
        }
    }
    let sessions: Vec<&MbxSession> = metrics
        .mbx
        .sessions
        .iter()
        .filter(|session| !session.probe_only)
        .collect();
    let lookups: u64 = sessions.iter().fold(0_u64, |total, session| {
        total.saturating_add(
            session
                .misses
                .saturating_add(session.not_looked_up)
                .saturating_add(session.bypassed),
        )
    });
    if metrics.mbx.store_primed
        && !metrics.mbx.seed_truncated
        && metrics.mbx.unparsed == 0
        && sessions.iter().all(|session| session.hits == 0)
        && lookups > 0
        && compiled_registry_unit(metrics)
    {
        violations.push(format!(
            "MBX object cache: 0 hits in {} sessions ({lookups} lookups missed or skipped) although the configured store held entries before the job",
            sessions.len()
        ));
    }
    if metrics.mbx.declared
        && sessions.is_empty()
        && metrics.mbx.unparsed == 0
        && metrics
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.artifacts)
            .any(|artifact| !artifact.fresh)
    {
        violations.push(
            "compiled without mbx: the checkout declares mbx (mise `mr_boxington` or the cargo wrapper) but the lane compiled units and recorded no `mbx[cache]` session"
                .to_owned(),
        );
    }
    let tools_hit = metrics
        .caches
        .iter()
        .any(|cache| cache.family == "tools" && cache.hit == Hit::Exact);
    if tools_hit && !metrics.toolchain_installs.is_empty() {
        violations.push(format!(
            "toolchain installed on an exact tools cache hit: {}",
            metrics.toolchain_installs.join(", ")
        ));
    }
    violations.extend(crate::observed_units::lane_repeats(metrics));
    violations
}
