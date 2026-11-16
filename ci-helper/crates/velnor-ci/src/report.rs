// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::telemetry::{Compilation, Invocation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: u32 = 1;
pub const SNAPSHOT: &str = "target/.velnor/restored-inputs.tsv";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    pub family: String,
    pub root: Option<String>,
    pub restore_step: String,
    pub save_step: String,
    #[serde(default)]
    pub requested_key: String,
    pub matched_key: Option<String>,
    pub hit: Hit,
    pub bytes: Option<u64>,
    pub usable: bool,
    pub usable_same_lock: bool,
    pub save: SaveOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveOutcome {
    NotAttempted,
    Saved,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hit {
    Exact,
    Fallback,
    Miss,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneMetrics {
    pub schema: u32,
    pub lane: String,
    pub caches: Vec<Cache>,
    pub invocations: Vec<Invocation>,
    pub downloaded: u64,
    pub first_cargo_seconds: Option<u64>,
    pub slowest_tests: Vec<(String, f64)>,
    pub tests_run: Option<u64>,
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub verification_errors: Vec<String>,
    pub violations: Vec<String>,
}

#[must_use]
pub fn waste(metrics: &LaneMetrics) -> Vec<String> {
    let mut violations = Vec::new();
    for invocation in &metrics.invocations {
        let warm = metrics.caches.iter().any(|cache| {
            cache.family == "target"
                && cache.root.as_deref() == Some(&invocation.root)
                && cache.usable
        });
        let same_lock = metrics.caches.iter().any(|cache| {
            cache.family == "target"
                && cache.root.as_deref() == Some(&invocation.root)
                && cache.usable_same_lock
        });
        let compiled: BTreeSet<&str> = metrics
            .invocations
            .iter()
            .filter(|other| {
                other.root == invocation.root && other.configuration == invocation.configuration
            })
            .flat_map(|other| &other.compilations)
            .map(|event| event.package.as_str())
            .collect();
        for event in &invocation.compilations {
            if (same_lock && !event.workspace)
                || (warm
                    && event.workspace
                    && (event.unchanged
                        && event
                            .dependencies
                            .iter()
                            .all(|dependency| !compiled.contains(dependency.as_str()))))
            {
                violations.push(format!(
                    "{}: {} v{} compiled despite usable restored inputs",
                    invocation.root, event.package, event.version
                ));
            }
        }
        if !invocation.downloads.is_empty()
            && metrics.caches.iter().any(|cache| {
                cache.family == "sources"
                    && cache.root.as_deref() == Some(&invocation.root)
                    && cache.hit == Hit::Exact
            })
        {
            violations.push(format!(
                "{}: downloaded on an exact sources hit: {}",
                invocation.root,
                invocation.downloads.join(", ")
            ));
        }
    }
    violations.extend(
        metrics
            .caches
            .iter()
            .filter(|cache| cache.save == SaveOutcome::Failed)
            .map(|cache| {
                format!(
                    "{} cache save failed: {}",
                    cache.family, cache.requested_key
                )
            }),
    );
    violations
}

pub fn check(lanes: &[LaneMetrics]) -> Result<String, CiError> {
    let observed_roots: BTreeSet<_> = lanes
        .iter()
        .flat_map(|lane| &lane.invocations)
        .filter(|invocation| !invocation.artifacts.is_empty() || invocation.rejected > 0)
        .map(|invocation| invocation.root.as_str())
        .collect();
    let mut identities = BTreeSet::new();
    let mut events = BTreeMap::new();
    let mut units = BTreeMap::new();
    let mut violations = Vec::new();
    for lane in lanes {
        validate_evidence(lane)?;
        if !identities.insert(&lane.lane) {
            return Err(CiError::new(format!(
                "duplicate metrics for lane {}",
                lane.lane
            )));
        }
        if lane.schema != SCHEMA {
            return Err(CiError::new(format!(
                "unsupported metrics schema {}",
                lane.schema
            )));
        }
        violations.extend(lane.violations.iter().cloned());
        violations.extend(waste(lane));
        for invocation in &lane.invocations {
            crate::observed_units::collect(invocation, &mut units);
            if observed_roots.contains(invocation.root.as_str()) {
                continue;
            }
            for event in &invocation.compilations {
                *events
                    .entry((
                        &invocation.root,
                        &invocation.configuration,
                        &event.package,
                        &event.version,
                    ))
                    .or_insert(0_u64) += 1;
            }
        }
    }
    violations.extend(events.into_iter().filter(|(_, count)| *count > 2).map(
        |((root, config, package, version), count)| {
            format!("{root}: {package} v{version} compiled {count} times for {config}")
        },
    ));
    violations.extend(units.into_iter().filter(|(_, (_, count))| *count > 1).map(
        |(_, (package, count))| {
            format!("{package}: identical observed compilation unit executed {count} times")
        },
    ));
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        Ok(format!("Cache: {} lanes checked, no waste\n", lanes.len()))
    } else {
        Err(CiError::new(violations.join("\n")))
    }
}

impl LaneMetrics {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut text = vec!["| Cache | Requested key | Matched key | Hit | Archive bytes | Save |\n|---|---|---|---|---|---|\n".to_owned()];
        for cache in &self.caches {
            text.push(format!(
                "| {} | `{}` | `{}` | {:?} | {} | {:?} |\n",
                cache.family,
                cache.requested_key,
                cache.matched_key.as_deref().unwrap_or("none"),
                cache.hit,
                cache
                    .bytes
                    .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string()),
                cache.save
            ));
        }
        let events: Vec<&Compilation> = self
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.compilations)
            .collect();
        let workspace = events.iter().filter(|event| event.workspace).count();
        text.push(format!("\nCompiled: {workspace} workspace, {} dependency crates. Downloaded: {}. tests_run: {:?}. Job start to first cargo: {:?} seconds.\n\n| Cargo root | Invocation | Seconds |\n|---|---|---|\n", events.len() - workspace, self.downloaded, self.tests_run, self.first_cargo_seconds));
        for invocation in &self.invocations {
            text.push(format!(
                "| {} | `{}` | {:.3} |\n",
                invocation.root,
                invocation.argv.join(" "),
                invocation.seconds
            ));
        }
        text.push("\n| Slowest tests | Seconds |\n|---|---|\n".to_owned());
        for (test, seconds) in &self.slowest_tests {
            text.push(format!("| {test} | {seconds:.3} |\n"));
        }
        for diagnostic in self
            .diagnostics
            .iter()
            .chain(&self.verification_errors)
            .chain(&self.violations)
        {
            text.push(format!("\n{diagnostic}\n"));
        }
        text.join("")
    }
}

fn validate_evidence(lane: &LaneMetrics) -> Result<(), CiError> {
    if let Some(cache) = lane
        .caches
        .iter()
        .find(|cache| cache.save == SaveOutcome::Unknown)
    {
        return Err(CiError::new(format!(
            "cannot verify cache save for lane {}: {}",
            lane.lane, cache.requested_key
        )));
    }
    if !lane.verification_errors.is_empty() {
        return Err(CiError::new(format!(
            "cannot evaluate cache waste for lane {}: {}",
            lane.lane,
            lane.verification_errors.join("; ")
        )));
    }
    for invocation in &lane.invocations {
        if !invocation.capture_complete || invocation.rejected > 0 {
            return Err(CiError::new(format!(
                "cannot evaluate cache waste for lane {} root {}: capture complete={}, rejected artifact records={}",
                lane.lane, invocation.root, invocation.capture_complete, invocation.rejected
            )));
        }
    }
    Ok(())
}
