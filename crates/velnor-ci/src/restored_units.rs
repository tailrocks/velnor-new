// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::cache_prune::{artifact_hash, is_hash};
use crate::cargo_package::PackageIdentity;
use crate::report::LaneMetrics;
use crate::telemetry::Invocation;

const HEADER: &str = "velnor-restored-units 1";

#[must_use]
pub fn encode(hashes: &BTreeSet<String>) -> String {
    let mut text = format!("{HEADER}\n");
    for hash in hashes {
        text.push_str(hash);
        text.push('\n');
    }
    text
}

#[must_use]
pub fn decode(text: &str) -> Option<BTreeSet<String>> {
    let mut lines = text.lines();
    (lines.next()? == HEADER)
        .then(|| {
            lines
                .map(|line| is_hash(line).then(|| line.to_owned()))
                .collect()
        })
        .flatten()
}

pub fn classify(invocation: &mut Invocation, restored: &BTreeSet<String>) {
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
            compilation.restored = Some(units.iter().any(|hash| restored.contains(*hash)));
        }
    }
}

#[must_use]
pub fn base_gaps(metrics: &LaneMetrics) -> Vec<String> {
    let mut gaps: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
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
            gaps.entry(&invocation.root)
                .or_default()
                .insert(format!("{} v{}", event.package, event.version));
        }
    }
    gaps.into_iter()
        .map(|(root, packages)| {
            format!(
                "{root}: dependency packages not in the restored base (a different package or feature selection than the run that saved it): {}",
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
