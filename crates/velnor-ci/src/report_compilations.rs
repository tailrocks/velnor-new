// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::report::LaneMetrics;
use crate::telemetry::Invocation;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compilation_repeats(lanes: &[LaneMetrics]) -> Result<Vec<String>, CiError> {
    let mut events = BTreeMap::new();
    let mut unknown = BTreeMap::new();
    for invocation in lanes.iter().flat_map(|lane| &lane.invocations) {
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
    violations.extend(crate::observed_units::repeats(lanes));
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
