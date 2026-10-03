use super::*;
use mbx_cache_core::{
    CacheDigest, FileIdentity, OutputObservation, PackageOrigin, ProcessMeasurement,
    ProcessOutcome, UnitIdentity,
};
use serde_json::json;

struct Fixture {
    _directory: tempfile::TempDir,
    artifact: CargoArtifactObservation,
    unit: UnitMeasurement,
}

impl Fixture {
    fn new(outcome: CacheOutcome, process: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("Cargo.toml");
        let source = directory.path().join("lib.rs");
        let output = directory.path().join("actual.rlib");
        std::fs::write(&manifest, "actual manifest").unwrap();
        std::fs::write(&source, "pub fn value() {}").unwrap();
        std::fs::write(&output, "actual observed output").unwrap();
        let manifest = manifest.canonicalize().unwrap();
        let source = source.canonicalize().unwrap();
        let output = output.canonicalize().unwrap();
        let artifact = serde_json::from_value(json!({
            "package_id":"fixture package", "manifest_path":manifest,
            "target":{"name":"fixture","kind":["lib"],"crate_types":["rlib"],"src_path":source,"edition":"2021"},
            "profile":{"opt_level":"0","debuginfo":0,"debug_assertions":true,"overflow_checks":true,"test":false},
            "features":[],"filenames":[output],"executable":null,"fresh":false,
        })).unwrap();
        let mut unit = UnitMeasurement {
            identity: Some(UnitIdentity {
                cargo_unit_id: Some("observed unit".into()),
                package_id: Some("fixture package".into()),
                manifest_path: Some(manifest),
                source_path: Some(source),
                provenance: UnitProvenance::CargoTarget,
                origin: PackageOrigin::Registry,
                source_origin: PackageOrigin::Registry,
            }),
            ..Default::default()
        };
        unit.invocations
            .entry(InvocationKind::Work)
            .or_default()
            .insert(outcome, 1);
        if process {
            unit.subprocesses
                .entry(ProcessPurpose::Work)
                .or_default()
                .insert(
                    ProcessOutcome::Succeeded,
                    ProcessMeasurement {
                        attempts: 1,
                        started: 1,
                        observed_wall_ns: 10,
                        wall_observations: 1,
                    },
                );
        }
        unit.outputs.push(OutputObservation {
            path: output.clone(),
            aliases: Vec::new(),
            cache_outcome: outcome,
            digest: CacheDigest::blake3_file(&output).unwrap(),
            file_identity: FileIdentity::for_digest_cache(
                &output,
                &std::fs::metadata(&output).unwrap(),
            )
            .unwrap(),
        });
        Self {
            _directory: directory,
            artifact,
            unit,
        }
    }
}

#[test]
fn actual_output_relation_matches_miss_and_native_hit_without_process() {
    for (outcome, process) in [(CacheOutcome::Miss, true), (CacheOutcome::Hit, false)] {
        let fixture = Fixture::new(outcome, process);
        assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), Some(0));
    }
}

#[test]
fn cargo_fresh_contradicts_both_miss_work_and_native_hit_restoration() {
    for (outcome, process) in [(CacheOutcome::Miss, true), (CacheOutcome::Hit, false)] {
        let mut fixture = Fixture::new(outcome, process);
        fixture.artifact.fresh = true;
        assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
    }
}

#[test]
fn duplicate_cargo_or_native_output_paths_are_not_collapsed() {
    let mut fixture = Fixture::new(CacheOutcome::Miss, true);
    fixture
        .artifact
        .filenames
        .push(fixture.artifact.filenames[0].clone());
    assert!(!outputs_agree(&fixture.unit, &fixture.artifact));
    fixture.artifact.filenames.pop();
    fixture.unit.outputs.push(fixture.unit.outputs[0].clone());
    assert!(!outputs_agree(&fixture.unit, &fixture.artifact));
}

#[test]
fn multiple_work_invocations_cannot_share_one_unbound_artifact() {
    let mut fixture = Fixture::new(CacheOutcome::Miss, true);
    fixture
        .unit
        .invocations
        .get_mut(&InvocationKind::Work)
        .unwrap()
        .insert(CacheOutcome::Miss, 2);
    assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
    fixture
        .unit
        .invocations
        .get_mut(&InvocationKind::Work)
        .unwrap()
        .insert(CacheOutcome::Miss, 1);
    fixture
        .unit
        .invocations
        .get_mut(&InvocationKind::Work)
        .unwrap()
        .insert(CacheOutcome::Hit, 1);
    assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
}

#[test]
fn orphan_process_unknown_outcome_and_replaced_output_cannot_join() {
    let mut fixture = Fixture::new(CacheOutcome::Miss, true);
    fixture.unit.invocations.clear();
    assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
    fixture
        .unit
        .invocations
        .entry(InvocationKind::Work)
        .or_default()
        .insert(CacheOutcome::Unknown, 1);
    assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
    fixture.unit.invocations.clear();
    fixture
        .unit
        .invocations
        .entry(InvocationKind::Work)
        .or_default()
        .insert(CacheOutcome::Miss, 1);
    std::fs::write(&fixture.artifact.filenames[0], "changed output").unwrap();
    assert_eq!(matching_unit(&fixture.unit, &[&fixture.artifact]), None);
}
