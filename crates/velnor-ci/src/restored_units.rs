// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::cache_prune::{artifact_hash, is_hash};
use crate::cargo_package::PackageIdentity;
use crate::report::LaneMetrics;
use crate::telemetry::{Compilation, Invocation};

const HEADER: &str = "velnor-restored-units 2";
const MISSING_INPUT: &str = "missing-input ";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoredUnits {
    pub present: BTreeSet<String>,
    pub missing_input: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manifest {
    Present,
    Absent,
    Unreadable,
}

#[must_use]
pub fn encode(units: &RestoredUnits) -> String {
    let mut text = format!("{HEADER}\n");
    for hash in &units.present {
        text.push_str(hash);
        text.push('\n');
    }
    for hash in &units.missing_input {
        text.push_str(MISSING_INPUT);
        text.push_str(hash);
        text.push('\n');
    }
    text
}

#[must_use]
pub fn decode(text: &str) -> Option<RestoredUnits> {
    let mut lines = text.lines();
    if lines.next()? != HEADER {
        return None;
    }
    let mut units = RestoredUnits::default();
    for line in lines {
        match line.strip_prefix(MISSING_INPUT) {
            Some(hash) if is_hash(hash) => units.missing_input.insert(hash.to_owned()),
            None if is_hash(line) => units.present.insert(line.to_owned()),
            _ => return None,
        };
    }
    Some(units)
}

pub fn classify(invocation: &mut Invocation, restored: &RestoredUnits) {
    for compilation in invocation
        .compilations
        .iter_mut()
        .filter(|compilation| !compilation.workspace)
    {
        let units: BTreeSet<&str> = invocation
            .artifacts
            .iter()
            .filter(|artifact| !artifact.fresh)
            .filter(|artifact| {
                PackageIdentity::parse(&artifact.package_id).is_some_and(|package| {
                    package.name == compilation.package && package.version == compilation.version
                })
            })
            .flat_map(|artifact| &artifact.filenames)
            .filter_map(|file| artifact_hash(file))
            .collect();
        if !units.is_empty() {
            let held = units.iter().any(|hash| restored.present.contains(*hash));
            compilation.restored = Some(held);
            compilation.missing_input = !held
                && units
                    .iter()
                    .any(|hash| restored.missing_input.contains(*hash));
        }
    }
}

pub fn attach_dirty(invocation: &mut Invocation) {
    for compilation in &mut invocation.compilations {
        let mut reasons = invocation
            .dirty
            .iter()
            .filter(|unit| {
                unit.package == compilation.package && unit.version == compilation.version
            })
            .map(|unit| unit.reason.as_str())
            .peekable();
        let first = reasons.peek().copied();
        let cause = reasons
            .find(|reason| !reason.contains("StaleDepFingerprint"))
            .or(first);
        compilation.dirty = cause.map(str::to_owned);
    }
}

#[must_use]
pub fn base_gaps(metrics: &LaneMetrics) -> Vec<String> {
    let mut gaps: BTreeMap<(&str, bool), BTreeSet<String>> = BTreeMap::new();
    for invocation in &metrics.invocations {
        if invocation.restored_target_usable == Some(false) || !same_lock(metrics, &invocation.root)
        {
            continue;
        }
        for event in invocation
            .compilations
            .iter()
            .filter(|event| !event.workspace && event.restored == Some(false))
        {
            gaps.entry((&invocation.root, event.missing_input))
                .or_default()
                .insert(format!("{} v{}", event.package, event.version));
        }
    }
    gaps.into_iter()
        .map(|((root, missing_input), packages)| {
            let packages = packages.into_iter().collect::<Vec<_>>().join(", ");
            if missing_input {
                format!(
                    "{root}: dependency packages whose restored dep-info names a file outside target/ that is missing (for example mbx build-script outputs in a cache directory this job did not have; MBX_SHARE_OUT_DIR=0 keeps them under target/): {packages}"
                )
            } else {
                format!(
                    "{root}: dependency packages not in the restored base (a different package or feature selection than the run that saved it): {packages}"
                )
            }
        })
        .collect()
}

#[must_use]
pub fn cargo_reason(compilation: &Compilation) -> String {
    compilation
        .dirty
        .as_ref()
        .map_or_else(String::new, |reason| format!(" (cargo: {reason})"))
}

#[must_use]
pub fn unknown_restores(metrics: &LaneMetrics) -> Vec<String> {
    let mut unknown: BTreeMap<(&str, Option<Manifest>), BTreeSet<String>> = BTreeMap::new();
    for invocation in &metrics.invocations {
        if invocation.restored_target_usable == Some(false) || !same_lock(metrics, &invocation.root)
        {
            continue;
        }
        for event in invocation
            .compilations
            .iter()
            .filter(|event| !event.workspace && event.restored.is_none())
        {
            unknown
                .entry((&invocation.root, invocation.restored_manifest))
                .or_default()
                .insert(format!(
                    "{} v{}{}",
                    event.package,
                    event.version,
                    cargo_reason(event)
                ));
        }
    }
    unknown
        .into_iter()
        .map(|((root, manifest), packages)| {
            let (level, state) = match manifest {
                Some(Manifest::Present) => ("notice", "present"),
                Some(Manifest::Absent) => ("warning", "absent"),
                Some(Manifest::Unreadable) => ("warning", "unreadable"),
                None => ("warning", "not recorded"),
            };
            format!(
                "::{level}::{root}: dependency compiles with an unknown restore status (restored-units manifest {state}): {}",
                packages.into_iter().collect::<Vec<_>>().join(", ")
            )
        })
        .collect()
}

fn same_lock(metrics: &LaneMetrics, root: &str) -> bool {
    metrics.caches.iter().any(|cache| {
        cache.family == "target" && cache.root.as_deref() == Some(root) && cache.usable_same_lock
    })
}
