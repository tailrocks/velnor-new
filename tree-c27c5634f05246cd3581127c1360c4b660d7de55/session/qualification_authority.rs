//! Exact fresh package attribution; no labels or directory-prefix inference.
use mbx_cache_core::{CompletedMeasurement, PackageOrigin, UnitProvenance};
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "qualification_authority_tests.rs"]
mod tests;

pub(super) fn packages_agree(measurement: &CompletedMeasurement) -> bool {
    let packages = &measurement.measurement_packages;
    let mut ids = BTreeSet::new();
    if packages.is_empty()
        || packages.iter().any(|package| {
            package.package_id.is_empty()
                || !ids.insert(&package.package_id)
                || package.origin == PackageOrigin::Unknown
                || !package.manifest_path.is_absolute()
                || package.sources.is_empty()
                || package.sources.iter().any(|source| !source.is_absolute())
        })
    {
        return false;
    }
    measurement.adapters.values().all(|adapter| {
        adapter.omitted_unit_events == 0
            && adapter.units.iter().all(|unit| {
                let Some(identity) = &unit.identity else {
                    return false;
                };
                let matches: Vec<_> = packages
                    .iter()
                    .filter(|package| {
                        Some(&package.package_id) == identity.package_id.as_ref()
                            && Some(&package.manifest_path) == identity.manifest_path.as_ref()
                            && package.origin == identity.origin
                    })
                    .collect();
                if matches.len() != 1 {
                    return false;
                }
                match identity.provenance {
                    UnitProvenance::CargoTarget => {
                        identity.cargo_unit_id.is_some()
                            && identity.source_origin == matches[0].origin
                            && identity
                                .source_path
                                .as_ref()
                                .is_some_and(|source| matches[0].sources.contains(source))
                    }
                    UnitProvenance::CargoPackageContext => {
                        identity.source_origin == PackageOrigin::Unknown
                            && identity
                                .source_path
                                .as_ref()
                                .is_some_and(|source| source.is_absolute())
                    }
                }
            })
    })
}

#[cfg(test)]
fn probe_only(unit: &mbx_cache_core::UnitMeasurement) -> bool {
    use mbx_cache_core::{InvocationKind, ProcessPurpose};
    unit.outputs.is_empty()
        && (!unit.invocations.is_empty() || !unit.subprocesses.is_empty())
        && unit
            .invocations
            .keys()
            .all(|kind| *kind == InvocationKind::Probe)
        && unit
            .subprocesses
            .keys()
            .all(|purpose| *purpose == ProcessPurpose::Probe)
}
