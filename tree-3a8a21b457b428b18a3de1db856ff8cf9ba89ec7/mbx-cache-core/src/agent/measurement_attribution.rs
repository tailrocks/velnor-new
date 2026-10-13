//! Exact Cargo-resolved package enrichment for observed native unit evidence.
use crate::{
    AdapterKind, AdapterMeasurement, MeasurementPackageAvailability, MeasurementPackageIdentity,
    PackageOrigin, UnitProvenance,
};
use eyre::{Result, bail};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

type PackageIndex = BTreeMap<PathBuf, MeasurementPackageIdentity>;
#[derive(Default)]
pub(super) struct PackageSnapshot {
    pub(super) generation: Option<u64>,
    pub(super) availability: Option<MeasurementPackageAvailability>,
    index: PackageIndex,
}
const MAX_PACKAGES: usize = 16_384;
const MAX_SOURCES: usize = 4096;

pub(super) fn record(
    snapshot: &mut PackageSnapshot,
    generation: u64,
    availability: MeasurementPackageAvailability,
    packages: Vec<MeasurementPackageIdentity>,
) -> Result<()> {
    if snapshot
        .generation
        .is_some_and(|current| generation <= current)
    {
        bail!("measurement package snapshot generation did not advance");
    }
    // A new failed or unavailable observation supersedes earlier authority.
    // Publish an unavailable replacement first; failed validation cannot leave stale data.
    *snapshot = PackageSnapshot {
        generation: Some(generation),
        availability: Some(MeasurementPackageAvailability::Unavailable),
        index: PackageIndex::new(),
    };
    if availability == MeasurementPackageAvailability::Unavailable {
        if !packages.is_empty() {
            bail!("unavailable package snapshot contains packages");
        }
        return Ok(());
    }
    let index = validated_index(packages)?;
    snapshot.index = index;
    snapshot.availability = Some(MeasurementPackageAvailability::Available);
    Ok(())
}

fn validated_index(packages: Vec<MeasurementPackageIdentity>) -> Result<PackageIndex> {
    if packages.len() > MAX_PACKAGES {
        bail!("measurement package batch exceeds bounds");
    }
    let mut index = BTreeMap::new();
    for mut package in packages {
        validate(&package)?;
        package.sources.sort();
        package.sources.dedup();
        if let Some(existing) = index.get(&package.manifest_path)
            && existing != &package
        {
            bail!("measurement package identity conflicts at exact manifest path");
        }
        index.insert(package.manifest_path.clone(), package);
    }
    Ok(index)
}

fn validate(package: &MeasurementPackageIdentity) -> Result<()> {
    if package.package_id.is_empty()
        || package.package_id.len() > 4096
        || package.sources.len() > MAX_SOURCES
    {
        bail!("measurement package identity exceeds bounds");
    }
    validate_path(&package.manifest_path)?;
    for source in &package.sources {
        validate_path(source)?;
    }
    Ok(())
}

pub(super) fn validate_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        bail!("measurement package path is not a bounded absolute path");
    }
    Ok(())
}

pub(super) fn enrich(
    adapters: &mut BTreeMap<AdapterKind, AdapterMeasurement>,
    snapshot: &PackageSnapshot,
) {
    for adapter in adapters.values_mut() {
        for unit in &mut adapter.units {
            let Some(identity) = unit.identity.as_mut() else {
                continue;
            };
            let observed_package_id = identity.package_id.take();
            identity.origin = PackageOrigin::Unknown;
            identity.source_origin = PackageOrigin::Unknown;
            let Some(manifest) = identity.manifest_path.as_ref() else {
                continue;
            };
            let Some(package) = snapshot.index.get(manifest) else {
                continue;
            };
            if identity.provenance == UnitProvenance::CargoTarget
                && !identity
                    .source_path
                    .as_ref()
                    .is_some_and(|source| package.sources.contains(source))
            {
                continue;
            }
            // A previously assigned package must agree with the resolved source.
            if observed_package_id
                .as_ref()
                .is_some_and(|id| id != &package.package_id)
            {
                continue;
            }
            identity.package_id = Some(package.package_id.clone());
            identity.origin = package.origin;
            identity.source_origin = if identity.provenance == UnitProvenance::CargoTarget {
                package.origin
            } else {
                PackageOrigin::Unknown
            };
        }
        adapter
            .units
            .sort_by(|left, right| left.identity.cmp(&right.identity));
    }
}

pub(super) struct MeasurementSnapshot {
    pub(super) adapters: BTreeMap<AdapterKind, AdapterMeasurement>,
    pub(super) generation: Option<u64>,
    pub(super) availability: Option<MeasurementPackageAvailability>,
    pub(super) packages: Vec<MeasurementPackageIdentity>,
}
impl super::CacheAgent {
    pub(super) fn measurement_snapshot(&self) -> MeasurementSnapshot {
        let packages = self
            .stats
            .measurement_packages
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut adapters = super::measurement::snapshot(&self.stats.measurement_adapters);
        enrich(&mut adapters, &packages);
        MeasurementSnapshot {
            adapters,
            generation: packages.generation,
            availability: packages.availability,
            packages: packages.index.values().cloned().collect(),
        }
    }
}

#[cfg(test)]
#[path = "measurement_attribution_tests.rs"]
mod tests;
