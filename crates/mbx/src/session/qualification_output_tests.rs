use super::*;
use mbx_cache_core::{CacheDigest, OutputObservation};

fn event() -> MeasurementEvent {
    MeasurementEvent::Output {
        adapter: AdapterKind::Rustc,
        unit: None,
        observation: OutputObservation {
            path: "/tmp/native-output.rlib".into(),
            aliases: Vec::new(),
            cache_outcome: CacheOutcome::Miss,
            digest: CacheDigest::blake3(b"actual compiler output"),
            file_identity: None,
        },
    }
}

#[test]
fn output_receipt_fidelity_does_not_increment_process_counts() {
    let mut expected = BTreeMap::new();
    add(&mut expected, &event()).unwrap();
    let row = &expected[&AdapterKind::Rustc];
    assert!(row.invocations.is_empty());
    assert!(row.subprocesses.is_empty());
    assert_eq!(row.units[0].outputs.len(), 1);
    assert!(check(&expected, &expected).is_ok());
}

#[test]
fn moved_or_changed_output_never_matches_terminal_receipt() {
    let mut expected = BTreeMap::new();
    add(&mut expected, &event()).unwrap();
    let mut actual = expected.clone();
    actual.get_mut(&AdapterKind::Rustc).unwrap().units[0].outputs[0].path =
        "/tmp/moved.rlib".into();
    assert_eq!(
        check(&actual, &expected),
        Err(QualificationReason::UnitAggregateMismatch)
    );
    actual = expected.clone();
    actual.get_mut(&AdapterKind::Rustc).unwrap().units[0].outputs[0].digest =
        CacheDigest::blake3(b"tampered");
    assert_eq!(
        check(&actual, &expected),
        Err(QualificationReason::UnitAggregateMismatch)
    );
}

#[test]
fn duplicate_output_mass_cannot_match_single_receipt() {
    let mut expected = BTreeMap::new();
    add(&mut expected, &event()).unwrap();
    let mut actual = expected.clone();
    actual.get_mut(&AdapterKind::Rustc).unwrap().units[0]
        .outputs
        .push(expected[&AdapterKind::Rustc].units[0].outputs[0].clone());
    assert_eq!(
        check(&actual, &expected),
        Err(QualificationReason::UnitAggregateMismatch)
    );
}
