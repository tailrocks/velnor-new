// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::report::{Hit, LaneMetrics};
use serde::{Deserialize, Serialize};

pub const TOOLCHAIN_INSTALLS: &str = "velnor-toolchain-installs.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxSession {
    pub hits: u64,
    pub misses: u64,
    pub not_looked_up: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MbxEvidence {
    #[serde(default)]
    pub declared: bool,
    pub store_primed: bool,
    pub sessions: Vec<MbxSession>,
    #[serde(default)]
    pub unparsed: u64,
}

impl MbxEvidence {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.declared && !self.store_primed && self.sessions.is_empty() && self.unparsed == 0
    }
}

fn lookup_summary(line: &str) -> Option<&str> {
    let text = line.split_once("mbx[cache]: ")?.1;
    let text = text.strip_prefix("object cache: ").unwrap_or(text);
    (text.starts_with(|c: char| c.is_ascii_digit()) && text.contains(" hits")).then_some(text)
}

fn parse_lookup_summary(summary: &str) -> Option<MbxSession> {
    let counts = summary.split_once(';').map_or(summary, |(head, _)| head);
    let mut session = MbxSession {
        hits: 0,
        misses: 0,
        not_looked_up: 0,
    };
    let mut seen = [false; 2];
    for clause in counts.split(',') {
        let (count, name) = clause.trim().split_once(' ')?;
        let count = count.parse().ok()?;
        match name.trim() {
            "hits" => (session.hits, seen[0]) = (count, true),
            "misses" => (session.misses, seen[1]) = (count, true),
            "not looked up" => session.not_looked_up = count,
            _ => {}
        }
    }
    (seen[0] && seen[1]).then_some(session)
}

#[must_use]
pub fn mbx_sessions(log: &[u8]) -> Vec<MbxSession> {
    String::from_utf8_lossy(log)
        .lines()
        .filter_map(|line| parse_lookup_summary(lookup_summary(line)?))
        .collect()
}

#[must_use]
pub fn mbx_unparsed_lines(log: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(log)
        .lines()
        .filter(|line| {
            lookup_summary(line).is_some_and(|summary| parse_lookup_summary(summary).is_none())
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
    let lookups: u64 = sessions
        .iter()
        .map(|session| session.misses + session.not_looked_up)
        .sum();
    if metrics.mbx.store_primed
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
