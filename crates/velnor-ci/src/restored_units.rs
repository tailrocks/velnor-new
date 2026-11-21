// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::cache_prune::{artifact_hash, is_hash};
use crate::cargo_package::PackageIdentity;
use crate::report::LaneMetrics;
use crate::telemetry::Invocation;

const HEADER: &str = "velnor-restored-units 2";
const MISSING_INPUT: &str = "missing-input ";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoredUnits {
    pub present: BTreeSet<String>,
    pub missing_input: BTreeSet<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub hash: String,
    pub package: String,
    pub unit: String,
    pub dependencies: BTreeSet<String>,
}

pub fn cascade(units: &mut RestoredUnits, fingerprints: &[Fingerprint]) {
    loop {
        let gaps = gap_crates(units, fingerprints);
        let forced: Vec<String> = fingerprints
            .iter()
            .filter(|print| {
                !units.missing_input.contains(&print.hash)
                    && print
                        .dependencies
                        .iter()
                        .any(|name| gaps.contains(&crate_name(name)))
            })
            .map(|print| print.hash.clone())
            .collect();
        if forced.is_empty() {
            return;
        }
        for hash in forced {
            units.present.remove(&hash);
            units.missing_input.insert(hash);
        }
    }
}

fn gap_crates(units: &RestoredUnits, fingerprints: &[Fingerprint]) -> BTreeSet<String> {
    let (gaps, held): (Vec<_>, Vec<_>) = fingerprints
        .iter()
        .partition(|print| units.missing_input.contains(&print.hash));
    let held: BTreeSet<(String, &str)> = held
        .iter()
        .map(|print| (crate_name(&print.package), unit_kind(&print.unit)))
        .collect();
    gaps.iter()
        .filter(|print| !held.contains(&(crate_name(&print.package), unit_kind(&print.unit))))
        .map(|print| crate_name(&print.package))
        .collect()
}

fn unit_kind(unit: &str) -> &str {
    unit.strip_suffix(".json").unwrap_or(unit)
}

fn crate_name(package: &str) -> String {
    package.replace('-', "_")
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
                    "{root}: dependency packages whose restored dep-info names a file outside target/ that is missing (for example mbx build-script outputs in a cache directory this job did not have; MBX_SHARE_OUT_DIR=0 keeps them under target/), and packages that rebuild because such a dependency is rebuilt: {packages}"
                )
            } else {
                format!(
                    "{root}: dependency packages not in the restored base (a different package or feature selection than the run that saved it): {packages}"
                )
            }
        })
        .collect()
}

fn same_lock(metrics: &LaneMetrics, root: &str) -> bool {
    metrics.caches.iter().any(|cache| {
        cache.family == "target" && cache.root.as_deref() == Some(root) && cache.usable_same_lock
    })
}
