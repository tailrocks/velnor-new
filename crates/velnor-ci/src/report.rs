// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::telemetry::{Compilation, Invocation};
use crate::waste_evidence::MbxEvidence;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    pub family: String,
    pub root: Option<String>,
    pub restore_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_step: Option<String>,
    #[serde(default)]
    pub requested_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_key: Option<String>,
    pub matched_key: Option<String>,
    pub hit: Hit,
    pub bytes: Option<u64>,
    pub usable: bool,
    pub usable_same_lock: bool,
    pub save: SaveOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_generated: Option<String>,
    /// A miss on a key that the cache inventory already listed, on a ref this job may read, when
    /// the job started. `None` when the inventory was not consulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_before_job: Option<bool>,
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
    #[serde(default, skip_serializing_if = "MbxEvidence::is_empty")]
    pub mbx: MbxEvidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toolchain_installs: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub privacy_unavailable: bool,
    pub violations: Vec<String>,
}

#[must_use]
pub fn waste(metrics: &LaneMetrics) -> Vec<String> {
    let mut violations = Vec::new();
    if metrics.privacy_unavailable {
        violations.push(format!(
            "{}: the repository privacy lookup was unavailable, so the base caches of its private dependencies were neither restored nor saved; the Cache check needs a verdict of private, public or internal",
            metrics.lane
        ));
    }
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
            let workspace_waste = warm
                && event.workspace
                && event.unchanged
                && event
                    .dependencies
                    .iter()
                    .all(|dependency| !compiled.contains(dependency.as_str()))
                && !pruned_outputs_only(invocation, &event.package, &event.version);
            if (same_lock && !event.workspace && event.restored == Some(true)) || workspace_waste {
                violations.push(format!(
                    "{}: {} v{} compiled despite usable restored inputs{}",
                    invocation.root,
                    event.package,
                    event.version,
                    crate::restored_units::cargo_reason(event)
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
            .map(|cache| format!("{} cache save failed: {}", cache.family, save_key(cache))),
    );
    violations.extend(crate::waste_evidence::waste(metrics));
    violations
}

fn pruned_outputs_only(invocation: &Invocation, package: &str, version: &str) -> bool {
    let mut saw = false;
    for artifact in &invocation.artifacts {
        if artifact.fresh {
            continue;
        }
        let Some(identity) = crate::cargo_package::PackageIdentity::parse(&artifact.package_id)
        else {
            continue;
        };
        if identity.name != package
            || identity.version != version
            || identity
                .source
                .is_none_or(|source| !source.starts_with("path+"))
        {
            continue;
        }
        saw = true;
        if retained_build_script(artifact)
            || artifact.filenames.is_empty()
            || artifact
                .filenames
                .iter()
                .any(|file| !pruned_output(file, invocation.target_directory.as_deref()))
        {
            return false;
        }
    }
    saw
}

fn retained_build_script(artifact: &crate::cargo_artifacts::CargoArtifact) -> bool {
    artifact
        .target
        .kind
        .iter()
        .any(|kind| kind == "custom-build")
        || artifact.filenames.iter().any(|file| {
            let name = file_name(file);
            name.starts_with("build_script_") || name.starts_with("build-script-")
        })
}

fn pruned_output(file: &str, target_directory: Option<&str>) -> bool {
    let normalized = file.replace('\\', "/");
    let name = file_name(&normalized);
    if name.starts_with("build_script_") || name.starts_with("build-script-") {
        return false;
    }
    crate::cache_prune::is_executable(name) || doc_output(&normalized, target_directory)
}

fn doc_output(file: &str, target_directory: Option<&str>) -> bool {
    if let Some(dir) = target_directory {
        let dir = dir.replace('\\', "/");
        let prefix = dir.trim_end_matches('/');
        let Some(rest) = file.strip_prefix(prefix) else {
            return false;
        };
        let rest = rest.trim_start_matches('/');
        return rest == "doc" || rest.starts_with("doc/");
    }
    let parts: Vec<&str> = file.split('/').collect();
    parts
        .windows(2)
        .any(|pair| pair[0] == "target" && pair[1] == "doc")
        || file.starts_with("doc/")
}

fn file_name(file: &str) -> &str {
    file.rsplit(['/', '\\']).next().unwrap_or(file)
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
    violations.extend(crate::report_compilations::compilation_repeats(lanes)?);
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        Ok(format!(
            "Cache: {} lanes checked, no waste\n{}",
            lanes.len(),
            restore_notices(lanes)
        ))
    } else {
        Err(CiError::new(violations.join("\n")))
    }
}

#[must_use]
pub fn restore_notices(lanes: &[LaneMetrics]) -> String {
    let mut notices: Vec<(&'static str, String)> = Vec::new();
    for lane in lanes {
        notices.extend(
            crate::restored_units::base_gaps(lane)
                .into_iter()
                .map(|gap| ("notice", gap)),
        );
        notices.extend(crate::restored_units::unknown_restores(lane));
    }
    notices.sort();
    notices.dedup();
    notices
        .iter()
        .map(|(level, notice)| crate::escape::command_line(level, notice))
        .collect()
}

impl LaneMetrics {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut text = vec!["| Cache | Requested key | Matched key | Hit | Archive bytes | Save |\n|---|---|---|---|---|---|\n".to_owned()];
        for cache in &self.caches {
            text.push(format!(
                "| {} | `{}` | `{}` | {:?} | {} | {} |\n",
                cache.family,
                cache.requested_key,
                cache.matched_key.as_deref().unwrap_or("none"),
                cache.hit,
                cache
                    .bytes
                    .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string()),
                match &cache.not_generated {
                    Some(reason) => format!("not generated ({reason})"),
                    None => format!("{:?}", cache.save),
                }
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

fn save_key(cache: &Cache) -> &str {
    cache
        .saved_key
        .as_deref()
        .filter(|key| !key.is_empty())
        .unwrap_or(&cache.requested_key)
}

fn validate_evidence(lane: &LaneMetrics) -> Result<(), CiError> {
    if let Some(cache) = lane
        .caches
        .iter()
        .find(|cache| cache.save == SaveOutcome::Unknown)
    {
        return Err(CiError::new(format!(
            "cannot verify cache save for lane {}: {}",
            lane.lane,
            save_key(cache)
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
