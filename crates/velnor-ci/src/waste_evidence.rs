// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::report::{Hit, LaneMetrics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const TOOLCHAIN_INSTALLS: &str = "velnor-toolchain-installs.txt";

pub const SEED_TRUNCATED: &str = "velnor-mbx-seed-truncated";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifications: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bypasses: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub compiler: BTreeMap<String, MbxCompilerWork>,
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

fn cache_text(line: &str) -> Option<(bool, &str)> {
    let text = line.split_once("mbx[cache]: ")?.1;
    Some(match text.strip_prefix("object cache:") {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    })
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
    let counts = summary.split_once(';').map_or(summary, |(head, _)| head);
    let mut session = MbxSession::default();
    let mut seen = [false; 2];
    for clause in counts.split(',') {
        let (count, name) = clause.trim().split_once(' ')?;
        let count = count.parse().ok()?;
        match name.trim() {
            "hits" => (session.hits, seen[0]) = (count, true),
            "misses" => (session.misses, seen[1]) = (count, true),
            "not looked up" => session.not_looked_up = count,
            "bypassed" | "incremental" => {}
            _ => return None,
        }
    }
    (seen[0] && seen[1]).then_some(session)
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
    metrics
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.artifacts)
        .any(|artifact| {
            !artifact.fresh
                && crate::cargo_package::PackageIdentity::parse(&artifact.package_id)
                    .and_then(|package| package.source)
                    .is_some_and(|source| source.starts_with("registry+"))
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
    let sessions = &metrics.mbx.sessions;
    let lookups: u64 = sessions.iter().fold(0_u64, |total, session| {
        total.saturating_add(session.misses.saturating_add(session.not_looked_up))
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
        && metrics.mbx.sessions.is_empty()
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
