// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::telemetry::{Compilation, Invocation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: u32 = 1;

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

impl Cache {
    #[must_use]
    pub fn exact_sources_for(&self, root: &str) -> bool {
        self.family == "sources"
            && self.hit == Hit::Exact
            && self.root.as_deref().is_none_or(|owner| owner == root)
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_name: Option<String>,
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
        let warm = invocation.restored_target_usable != Some(false)
            && metrics.caches.iter().any(|cache| {
                cache.family == "target"
                    && cache.root.as_deref() == Some(&invocation.root)
                    && cache.usable
            });
        let same_lock = invocation.restored_target_usable != Some(false)
            && metrics.caches.iter().any(|cache| {
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
            .filter(|event| event.workspace)
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
            && metrics
                .caches
                .iter()
                .any(|cache| cache.exact_sources_for(&invocation.root))
        {
            violations.push(format!(
                "{}: downloaded on an exact sources hit: {}",
                invocation.root,
                invocation.downloads.join(", ")
            ));
        }
        if let Some(violation) = git_waste(metrics, invocation) {
            violations.push(violation);
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

fn git_waste(metrics: &LaneMetrics, invocation: &Invocation) -> Option<String> {
    let repositories = invocation.verified_git_downloads.as_ref()?;
    (!repositories.is_empty()
        && metrics
            .caches
            .iter()
            .any(|cache| cache.exact_sources_for(&invocation.root)))
    .then(|| {
        format!(
            "{}: Git sources fetched on an exact sources hit: {}",
            invocation.root,
            repositories.join(", ")
        )
    })
}

pub fn check(lanes: &[LaneMetrics]) -> Result<String, CiError> {
    let mut identities = BTreeSet::new();
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
    }
    violations.extend(compilation_repeats(lanes)?);
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        Ok(format!("Cache: {} lanes checked, no waste\n", lanes.len()))
    } else {
        Err(CiError::new(violations.join("\n")))
    }
}

fn compilation_repeats(lanes: &[LaneMetrics]) -> Result<Vec<String>, CiError> {
    let mut events = BTreeMap::new();
    let mut units = BTreeMap::new();
    let mut unknown = BTreeMap::new();
    for invocation in lanes.iter().flat_map(|lane| &lane.invocations) {
        crate::observed_units::collect(invocation, &mut units);
        let mut covered = BTreeSet::new();
        for artifact in invocation
            .artifacts
            .iter()
            .filter(|artifact| !artifact.fresh)
        {
            if let Some(package) =
                crate::cargo_package::PackageIdentity::parse(&artifact.package_id)
            {
                covered.insert((package.name, package.version));
            }
            if crate::observed_units::identity(invocation, artifact).is_none() {
                let key = unknown_context(invocation, artifact)?;
                let entry = unknown
                    .entry(key)
                    .or_insert_with(|| ((&invocation.root, &artifact.target.name), 0_u64));
                entry.1 += 1;
            }
        }
        for event in &invocation.compilations {
            if !covered.contains(&(event.package.as_str(), event.version.as_str())) {
                *events
                    .entry((
                        &invocation.root,
                        &invocation.configuration,
                        crate::telemetry::target_selection(&invocation.argv),
                        &invocation.execution,
                        &event.package,
                        &event.version,
                    ))
                    .or_insert(0_u64) += 1;
            }
        }
    }
    if let Some(((root, package), count)) = unknown.values().find(|(_, count)| *count > 2) {
        return Err(CiError::new(format!(
            "cannot evaluate cache waste for root {root} crate {package}: compilation identity remains unknown after {count} observations"
        )));
    }
    let mut violations = Vec::new();
    violations.extend(events.into_iter().filter(|(_, count)| *count > 2).map(
        |((root, config, _, _, package, version), count)| {
            format!("{root}: {package} v{version} compiled {count} times for {config}")
        },
    ));
    violations.extend(units.into_iter().filter(|(_, (_, count))| *count > 1).map(
        |(_, (package, count))| {
            format!("{package}: identical observed compilation unit executed {count} times")
        },
    ));
    Ok(violations)
}

fn unknown_context(
    invocation: &Invocation,
    artifact: &crate::cargo_artifacts::CargoArtifact,
) -> Result<String, CiError> {
    let mut features = artifact.features.clone();
    features.sort();
    features.dedup();
    let mut filenames = artifact.filenames.clone();
    filenames.sort();
    filenames.dedup();
    serde_json::to_string(&(
        &invocation.root,
        &invocation.configuration,
        &artifact.package_id,
        &artifact.target,
        &artifact.profile,
        features,
        filenames,
        &invocation.execution,
    ))
    .map_err(|error| CiError::new(format!("cannot encode compilation observation: {error}")))
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
        let git_fetches: Option<usize> = self
            .invocations
            .iter()
            .map(|invocation| invocation.verified_git_downloads.as_ref().map(Vec::len))
            .sum();
        text.push(format!(
            "\nGit source repositories fetched: {}. Git crate download count: unknown.\n",
            git_fetches.map_or_else(|| "unknown".to_owned(), |count| count.to_string())
        ));
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
