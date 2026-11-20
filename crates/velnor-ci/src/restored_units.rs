// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The units a restored base held, recorded when it was restored. A dependency compile is waste
//! only if the base held the unit that was compiled again. The save keeps just the units the run
//! that saved it used, so a later run with another package or feature selection compiles units
//! the base never held; that is a gap in the base, not a cache that failed to serve.

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

/// `None` for any text this helper did not write, so an unreadable record keeps the old rule.
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

/// Marks each dependency compilation of `invocation` with whether any of its non-fresh units
/// was in `restored`. A compilation whose units cannot be read stays unmarked.
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

/// Dependency units compiled that the restored base never held, by root. They are no waste: the
/// run that saved the base used other packages or features, so the base had nothing to serve.
#[must_use]
pub fn base_gaps(metrics: &LaneMetrics) -> Vec<String> {
    let mut gaps: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for invocation in &metrics.invocations {
        if invocation.restored_target_usable == Some(false) {
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
        .map(|(root, units)| {
            format!(
                "{root}: {} dependency units were not in the restored base (a different package or feature selection than the run that saved it): {}",
                units.len(),
                units.into_iter().collect::<Vec<_>>().join(", ")
            )
        })
        .collect()
}
