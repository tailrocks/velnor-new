use std::error::Error;

use crate::load::one_minute_milli;
use crate::memory::available_bytes;
use crate::pressure::memory_some_avg10;
use crate::record::{MAX_OUTPUT_BYTES, ProbeRecord};
use crate::units::{decimal_milli_ceil, percent_basis_points};

#[test]
fn decimal_load_uses_checked_thousandth_ceiling() -> Result<(), Box<dyn Error>> {
    assert_eq!(decimal_milli_ceil("0")?, 0);
    assert_eq!(decimal_milli_ceil("0.000")?, 0);
    assert_eq!(decimal_milli_ceil("0.0001")?, 1);
    assert_eq!(decimal_milli_ceil("3.125")?, 3125);
    assert_eq!(decimal_milli_ceil("3.1251")?, 3126);
    assert!(decimal_milli_ceil("18446744073709552").is_err());
    for invalid in ["", ".1", "1.", "+1", "-1", "1e2", "NaN"] {
        assert!(decimal_milli_ceil(invalid).is_err());
    }
    Ok(())
}

#[test]
fn loadavg_requires_the_documented_five_fields() -> Result<(), Box<dyn Error>> {
    assert_eq!(one_minute_milli(b"1.0001 0.50 0.25 1/90 1234\n")?, 1001);
    assert!(one_minute_milli(b"0.1 0.2 0.3 1/9\n").is_err());
    assert!(one_minute_milli(b"0.1 0.2 0.3 1/9 5 extra\n").is_err());
    assert!(one_minute_milli(b"-0.1 0.2 0.3 1/9 5\n").is_err());
    Ok(())
}

#[test]
fn memavailable_is_unique_checked_and_converted_from_kib() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        available_bytes(b"MemTotal: 10 kB\nMemAvailable: 4 kB\n")?,
        4096
    );
    assert_eq!(available_bytes(b"MemAvailable: 0 kB\n")?, 0);
    assert!(available_bytes(b"MemAvailable: 18446744073709551615 kB\n").is_err());
    assert!(available_bytes(b"MemAvailable: +2 kB\n").is_err());
    assert!(available_bytes(b"MemAvailable: 2 kB extra\n").is_err());
    assert!(available_bytes(b"MemAvailable: 2 kB\nMemAvailable: 3 kB\n").is_err());
    assert!(available_bytes(b"MemTotal: 3 kB\n").is_err());
    Ok(())
}

#[test]
fn psi_percent_converts_exactly_and_rejects_unrepresentable_values() -> Result<(), Box<dyn Error>> {
    assert_eq!(percent_basis_points("0")?, 0);
    assert_eq!(percent_basis_points("0.05")?, 5);
    assert_eq!(percent_basis_points("100.00")?, 10_000);
    assert!(percent_basis_points("100.01").is_err());
    assert!(percent_basis_points("0.001").is_err());
    assert!(percent_basis_points("1e1").is_err());
    Ok(())
}

#[test]
fn psi_is_optional_but_valid_some_line_is_strict() {
    let valid = b"some avg10=0.05 avg60=0.10 avg300=0.20 total=99\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    assert_eq!(memory_some_avg10(valid), Some(5));
    assert_eq!(memory_some_avg10(b""), None);
    assert_eq!(memory_some_avg10(b"unsupported\n"), None);
    assert_eq!(
        memory_some_avg10(b"some avg10=101.00 avg60=0.10 avg300=0.20 total=99\n"),
        None
    );
    assert_eq!(
        memory_some_avg10(b"some avg10=0.10 avg10=0.20 avg60=0.1 avg300=0.2 total=1\n"),
        None
    );
}

#[test]
fn record_json_has_stable_required_keys_and_nullable_psi() -> Result<(), Box<dyn Error>> {
    let record = ProbeRecord {
        schema_version: 1,
        docker_root_free_bytes: 0,
        memory_available_bytes: 2,
        load_milli: 3,
        memory_psi_some_avg10_bps: None,
    };
    let bytes = serde_json::to_vec(&record)?;
    assert_eq!(
        std::str::from_utf8(&bytes)?,
        r#"{"schema_version":1,"docker_root_free_bytes":0,"memory_available_bytes":2,"load_milli":3,"memory_psi_some_avg10_bps":null}"#
    );
    assert!(bytes.len() < MAX_OUTPUT_BYTES);
    Ok(())
}
