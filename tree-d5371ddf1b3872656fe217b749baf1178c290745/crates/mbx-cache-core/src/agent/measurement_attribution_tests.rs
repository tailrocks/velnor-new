use super::*;
use crate::{CacheOutcome, InvocationKind, UnitIdentity, UnitMeasurement};

fn absolute(path: &str) -> PathBuf {
    #[cfg(windows)]
    return PathBuf::from(format!("C:{path}"));
    #[cfg(not(windows))]
    PathBuf::from(path)
}

fn package() -> MeasurementPackageIdentity {
    MeasurementPackageIdentity {
        package_id: "registry+https://example.invalid#index#dep@1.0.0".into(),
        manifest_path: absolute("/cache/dep/Cargo.toml"),
        sources: vec![absolute("/cache/dep/src/lib.rs")],
        origin: PackageOrigin::Registry,
    }
}
fn adapters(identity: UnitIdentity) -> BTreeMap<AdapterKind, AdapterMeasurement> {
    BTreeMap::from([(
        AdapterKind::Rustc,
        AdapterMeasurement {
            invocations: BTreeMap::from([(
                InvocationKind::Work,
                BTreeMap::from([(CacheOutcome::Miss, 1)]),
            )]),
            units: vec![UnitMeasurement {
                identity: Some(identity),
                ..Default::default()
            }],
            ..Default::default()
        },
    )])
}
#[test]
fn exact_target_source_proves_package_and_source_origin() {
    let mut index = PackageSnapshot::default();
    record(
        &mut index,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    let mut measured = adapters(UnitIdentity {
        manifest_path: Some(package().manifest_path),
        source_path: Some(package().sources[0].clone()),
        ..Default::default()
    });
    enrich(&mut measured, &index);
    let identity = measured[&AdapterKind::Rustc].units[0]
        .identity
        .as_ref()
        .unwrap();
    assert_eq!(identity.origin, PackageOrigin::Registry);
    assert_eq!(identity.source_origin, PackageOrigin::Registry);
    assert_eq!(
        identity.package_id.as_deref(),
        Some(package().package_id.as_str())
    );
}
#[test]
fn native_context_never_claims_native_source_ownership() {
    let mut index = PackageSnapshot::default();
    record(
        &mut index,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    let mut measured = adapters(UnitIdentity {
        manifest_path: Some(package().manifest_path),
        source_path: Some(absolute("/workspace/vendor/native.c")),
        provenance: UnitProvenance::CargoPackageContext,
        ..Default::default()
    });
    enrich(&mut measured, &index);
    let identity = measured[&AdapterKind::Rustc].units[0]
        .identity
        .as_ref()
        .unwrap();
    assert_eq!(identity.origin, PackageOrigin::Registry);
    assert_eq!(identity.source_origin, PackageOrigin::Unknown);
    assert_eq!(
        identity.source_path.as_deref(),
        Some(absolute("/workspace/vendor/native.c").as_path())
    );
    assert_eq!(
        measured[&AdapterKind::Rustc].invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        1
    );
}
#[test]
fn mismatched_or_missing_target_source_remains_unknown() {
    let mut index = PackageSnapshot::default();
    record(
        &mut index,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    for source_path in [None, Some(absolute("/cache/dep/other.rs"))] {
        let mut measured = adapters(UnitIdentity {
            manifest_path: Some(package().manifest_path),
            source_path,
            ..Default::default()
        });
        enrich(&mut measured, &index);
        let identity = measured[&AdapterKind::Rustc].units[0]
            .identity
            .as_ref()
            .unwrap();
        assert!(identity.package_id.is_none());
        assert_eq!(identity.origin, PackageOrigin::Unknown);
    }
}
#[test]
fn conflicting_manifest_batch_changes_nothing() {
    let mut index = PackageSnapshot::default();
    let mut conflicting = package();
    conflicting.package_id = "other@1.0.0".into();
    assert!(
        record(
            &mut index,
            1,
            MeasurementPackageAvailability::Available,
            vec![package(), conflicting]
        )
        .is_err()
    );
    assert!(index.index.is_empty());
}
#[test]
fn relative_package_paths_are_rejected() {
    let mut index = PackageSnapshot::default();
    let mut relative = package();
    relative.manifest_path = PathBuf::from("dep/Cargo.toml");
    assert!(
        record(
            &mut index,
            1,
            MeasurementPackageAvailability::Available,
            vec![relative]
        )
        .is_err()
    );
    assert!(index.index.is_empty());
}

#[test]
fn newer_snapshot_replaces_changed_package_id_origin_and_sources() {
    let mut snapshot = PackageSnapshot::default();
    record(
        &mut snapshot,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    let mut newer = package();
    newer.package_id = "path+file:///workspace/package#2.0.0".into();
    newer.origin = PackageOrigin::Workspace;
    newer.sources = vec![absolute("/cache/dep/src/new.rs")];
    record(
        &mut snapshot,
        2,
        MeasurementPackageAvailability::Available,
        vec![newer.clone()],
    )
    .unwrap();
    let mut measured = adapters(UnitIdentity {
        manifest_path: Some(newer.manifest_path.clone()),
        source_path: Some(newer.sources[0].clone()),
        ..Default::default()
    });
    enrich(&mut measured, &snapshot);
    let unit = measured[&AdapterKind::Rustc].units[0]
        .identity
        .as_ref()
        .unwrap();
    assert_eq!(unit.package_id.as_deref(), Some(newer.package_id.as_str()));
    assert_eq!(unit.origin, PackageOrigin::Workspace);
    assert_eq!(snapshot.index.len(), 1);
}
#[test]
fn unavailable_postgraph_clears_authority_without_erasing_counts() {
    let mut snapshot = PackageSnapshot::default();
    record(
        &mut snapshot,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    let mut measured = adapters(UnitIdentity {
        package_id: Some(package().package_id),
        origin: PackageOrigin::Registry,
        source_origin: PackageOrigin::Registry,
        manifest_path: Some(package().manifest_path),
        source_path: Some(package().sources[0].clone()),
        ..Default::default()
    });
    record(
        &mut snapshot,
        2,
        MeasurementPackageAvailability::Unavailable,
        vec![],
    )
    .unwrap();
    enrich(&mut measured, &snapshot);
    let unit = measured[&AdapterKind::Rustc].units[0]
        .identity
        .as_ref()
        .unwrap();
    assert!(unit.package_id.is_none());
    assert_eq!(unit.origin, PackageOrigin::Unknown);
    assert_eq!(unit.source_origin, PackageOrigin::Unknown);
    assert_eq!(
        measured[&AdapterKind::Rustc].invocations[&InvocationKind::Work][&CacheOutcome::Miss],
        1
    );
    assert_eq!(
        snapshot.availability,
        Some(MeasurementPackageAvailability::Unavailable)
    );
}
#[test]
fn malformed_new_snapshot_invalidates_old_authority_atomically() {
    let mut snapshot = PackageSnapshot::default();
    record(
        &mut snapshot,
        1,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    let mut conflicting = package();
    conflicting.origin = PackageOrigin::Workspace;
    assert!(
        record(
            &mut snapshot,
            2,
            MeasurementPackageAvailability::Available,
            vec![package(), conflicting]
        )
        .is_err()
    );
    assert!(snapshot.index.is_empty());
    assert_eq!(snapshot.generation, Some(2));
    assert_eq!(
        snapshot.availability,
        Some(MeasurementPackageAvailability::Unavailable)
    );
}
#[test]
fn stale_generation_cannot_replace_newer_authority() {
    let mut snapshot = PackageSnapshot::default();
    record(
        &mut snapshot,
        2,
        MeasurementPackageAvailability::Available,
        vec![package()],
    )
    .unwrap();
    assert!(
        record(
            &mut snapshot,
            1,
            MeasurementPackageAvailability::Unavailable,
            vec![]
        )
        .is_err()
    );
    assert_eq!(snapshot.generation, Some(2));
    assert_eq!(
        snapshot.availability,
        Some(MeasurementPackageAvailability::Available)
    );
    assert_eq!(snapshot.index.len(), 1);
}
