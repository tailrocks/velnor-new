//! Join actual native outputs to the original owning public Cargo stream.
use crate::cargo_artifact_capture::{CargoArtifactObservation, CargoCaptureReport, CargoMessage};
use mbx_cache_core::{
    AdapterKind, CacheOutcome, CompletedMeasurement, InvocationKind, ProcessPurpose,
    UnitMeasurement, UnitProvenance,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[cfg(all(test, unix))]
#[path = "qualification_artifact_tests.rs"]
mod tests;

/// This relation is consumed only behind original native capture/dispatch proof.
/// `fresh` is crosschecked, never treated as native compilation or zero work.
pub(super) fn agrees(measurement: &CompletedMeasurement, capture: &CargoCaptureReport) -> bool {
    let artifacts: Vec<_> = capture
        .messages
        .iter()
        .filter_map(|message| match message {
            CargoMessage::CompilerArtifact(artifact) => Some(artifact),
            _ => None,
        })
        .collect();
    if artifacts.is_empty()
        || artifacts
            .iter()
            .any(|artifact| !package_agrees(measurement, artifact))
    {
        return false;
    }
    let units: Vec<_> = measurement
        .adapters
        .get(&AdapterKind::Rustc)
        .into_iter()
        .flat_map(|adapter| &adapter.units)
        .filter(|unit| {
            unit.invocations.contains_key(&InvocationKind::Work)
                || unit.subprocesses.contains_key(&ProcessPurpose::Work)
                || !unit.outputs.is_empty()
        })
        .collect();
    let mut matched = BTreeSet::new();
    for unit in units {
        let Some(index) = matching_unit(unit, &artifacts) else {
            return false;
        };
        if !matched.insert(index) {
            return false;
        }
    }
    // Nonfresh means Cargo invoked a wrapper, whose actual processes are an
    // independent native observation. An unmatched nonfresh row is unavailable.
    artifacts
        .iter()
        .enumerate()
        .all(|(index, artifact)| artifact.fresh || matched.contains(&index))
}

fn matching_unit(unit: &UnitMeasurement, artifacts: &[&CargoArtifactObservation]) -> Option<usize> {
    let outcomes = unit.invocations.get(&InvocationKind::Work)?;
    if outcomes.is_empty()
        || outcomes.values().any(|count| *count == 0)
        || outcomes.contains_key(&CacheOutcome::Unknown)
        || outcomes
            .values()
            .try_fold(0u64, |total, count| total.checked_add(*count))
            != Some(1)
    {
        return None;
    }
    let identity = unit.identity.as_ref()?;
    if identity.provenance != UnitProvenance::CargoTarget
        || unit.outputs.is_empty()
        || unit.omitted_output_events != 0
    {
        return None;
    }
    let candidates: Vec<_> = artifacts
        .iter()
        .enumerate()
        .filter(|(_, artifact)| {
            !artifact.fresh
                && Some(&artifact.package_id) == identity.package_id.as_ref()
                && std::fs::canonicalize(&artifact.manifest_path).ok().as_ref()
                    == identity.manifest_path.as_ref()
                && std::fs::canonicalize(&artifact.target.src_path)
                    .ok()
                    .as_ref()
                    == identity.source_path.as_ref()
                && outputs_agree(unit, artifact)
        })
        .map(|(index, _)| index)
        .collect();
    (candidates.len() == 1).then(|| candidates[0])
}

fn package_agrees(measurement: &CompletedMeasurement, artifact: &CargoArtifactObservation) -> bool {
    let paths: Option<BTreeSet<_>> = artifact
        .filenames
        .iter()
        .map(|path| std::fs::canonicalize(path).ok())
        .collect();
    if paths.is_none_or(|paths| paths.is_empty() || paths.len() != artifact.filenames.len()) {
        return false;
    }
    let Some(manifest) = std::fs::canonicalize(&artifact.manifest_path).ok() else {
        return false;
    };
    let Some(source) = std::fs::canonicalize(&artifact.target.src_path).ok() else {
        return false;
    };
    measurement
        .measurement_packages
        .iter()
        .filter(|package| {
            package.package_id == artifact.package_id
                && package.manifest_path == manifest
                && package.sources.contains(&source)
        })
        .count()
        == 1
}

fn outputs_agree(unit: &UnitMeasurement, artifact: &CargoArtifactObservation) -> bool {
    let expected: BTreeSet<PathBuf> = unit
        .outputs
        .iter()
        .map(|output| output.path.clone())
        .collect();
    let actual: Option<BTreeSet<PathBuf>> = artifact
        .filenames
        .iter()
        .map(|path| std::fs::canonicalize(path).ok())
        .collect();
    let Some(actual) = actual else { return false };
    expected.len() == unit.outputs.len()
        && actual.len() == artifact.filenames.len()
        && expected == actual
        && !actual.is_empty()
        && unit.outputs.iter().all(|output| {
            output.aliases.is_empty()
                && artifact
                    .filenames
                    .iter()
                    .any(|path| crate::unit_artifact_binding::matches_current_output(output, path))
        })
}
