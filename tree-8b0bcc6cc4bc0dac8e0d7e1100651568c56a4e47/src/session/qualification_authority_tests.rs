use super::*;
use mbx_cache_core::{AdapterKind, AdapterMeasurement, MeasurementPackageIdentity, UnitIdentity};
use mbx_cache_core::{InvocationKind, UnitMeasurement};

fn measurement() -> CompletedMeasurement {
    let package = MeasurementPackageIdentity {
        package_id: "actual resolved package".into(),
        manifest_path: "/repo/Cargo.toml".into(),
        sources: vec!["/repo/src/lib.rs".into()],
        origin: PackageOrigin::Registry,
    };
    let identity = UnitIdentity {
        cargo_unit_id: Some("actual-cargo-unit".into()),
        package_id: Some(package.package_id.clone()),
        manifest_path: Some(package.manifest_path.clone()),
        source_path: Some(package.sources[0].clone()),
        provenance: UnitProvenance::CargoTarget,
        origin: package.origin,
        source_origin: package.origin,
    };
    let mut adapter = AdapterMeasurement::default();
    adapter.units.push(UnitMeasurement {
        identity: Some(identity),
        ..Default::default()
    });
    CompletedMeasurement {
        measurement_packages: vec![package],
        adapters: [(AdapterKind::Rustc, adapter)].into(),
        coverage: Default::default(),
        measurement_package_generation: Some(1),
        measurement_package_availability: Some(
            mbx_cache_core::MeasurementPackageAvailability::Available,
        ),
        workload_wall_ns: None,
        cache_post_workload_drain_ns: None,
        link_wall_ns: None,
        link_attribution: mbx_cache_core::LinkAttribution::CombinedUnknown,
    }
}

#[test]
fn package_source_and_origin_require_exact_fresh_mapping() {
    let mut measurement = measurement();
    assert!(packages_agree(&measurement));
    measurement
        .adapters
        .get_mut(&AdapterKind::Rustc)
        .unwrap()
        .units[0]
        .identity
        .as_mut()
        .unwrap()
        .origin = PackageOrigin::Workspace;
    assert!(!packages_agree(&measurement));
    measurement = self::measurement();
    measurement
        .adapters
        .get_mut(&AdapterKind::Rustc)
        .unwrap()
        .units[0]
        .identity
        .as_mut()
        .unwrap()
        .source_path = Some("/repo/other.rs".into());
    assert!(!packages_agree(&measurement));
}

#[test]
fn native_package_context_does_not_claim_native_source_origin() {
    let mut measurement = measurement();
    let identity = measurement
        .adapters
        .get_mut(&AdapterKind::Rustc)
        .unwrap()
        .units[0]
        .identity
        .as_mut()
        .unwrap();
    identity.provenance = UnitProvenance::CargoPackageContext;
    identity.source_path = Some("/native/generated.c".into());
    identity.source_origin = PackageOrigin::Unknown;
    assert!(packages_agree(&measurement));
    measurement
        .adapters
        .get_mut(&AdapterKind::Rustc)
        .unwrap()
        .units[0]
        .identity
        .as_mut()
        .unwrap()
        .source_origin = PackageOrigin::Registry;
    assert!(!packages_agree(&measurement));
}

#[test]
fn only_explicit_probe_observations_can_have_unknown_unit_identity() {
    let mut row = UnitMeasurement::default();
    assert!(!probe_only(&row));
    row.invocations
        .entry(InvocationKind::Probe)
        .or_default()
        .insert(mbx_cache_core::CacheOutcome::Unknown, 1);
    assert!(probe_only(&row));
    row.invocations
        .entry(InvocationKind::Work)
        .or_default()
        .insert(mbx_cache_core::CacheOutcome::Miss, 1);
    assert!(!probe_only(&row));
}
