use super::{parse_record, validate_record};
use crate::error::HostError;

fn valid_record() -> Vec<u8> {
    b"{\"schema_version\":1,\"docker_root_free_bytes\":4,\"docker_root_total_bytes\":8,\"memory_available_bytes\":2,\"load_milli\":3,\"memory_psi_some_avg10_bps\":10000}\n".to_vec()
}

#[test]
fn accepts_only_the_canonical_bounded_record() -> Result<(), HostError> {
    let record = parse_record(&valid_record())?;
    validate_record(&record)
}

#[test]
fn rejects_unknown_duplicate_noncanonical_and_multiline_records() {
    let valid = valid_record();
    let unknown = valid
        .strip_suffix(b"\n")
        .map(|record| [b"{\"extra\":0,".as_slice(), &record[1..], b"\n"].concat());
    assert!(unknown.is_some_and(|bytes| parse_record(&bytes).is_err()));
    assert!(parse_record(b"{\"schema_version\":1,\"schema_version\":1}\n").is_err());
    let mut multiline = valid.clone();
    multiline.insert(1, b'\n');
    assert!(parse_record(&multiline).is_err());
    let noncanonical = valid
        .iter()
        .copied()
        .filter(|byte| *byte != b'\n')
        .collect::<Vec<_>>();
    assert!(parse_record(&noncanonical).is_err());
}

#[test]
fn rejects_invalid_schema_capacity_and_pressure() -> Result<(), HostError> {
    let mut record = parse_record(&valid_record())?;
    record.schema_version = 2;
    assert_eq!(validate_record(&record), Err(HostError::Identity));
    record.schema_version = 1;
    record.docker_root_free_bytes = 9;
    assert_eq!(validate_record(&record), Err(HostError::Identity));
    record.docker_root_free_bytes = 4;
    record.memory_psi_some_avg10_bps = Some(10_001);
    assert_eq!(validate_record(&record), Err(HostError::Identity));
    Ok(())
}
