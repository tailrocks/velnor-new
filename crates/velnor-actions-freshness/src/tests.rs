//! Source authority and strict time parser regression cases.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{FreshnessContext, RUST_SOURCE_CAP, parse_iso_date, parse_timestamp};

fn source_fixture(source: &[u8]) -> Option<(PathBuf, FreshnessContext)> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-freshness-source-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir_all(&root).ok()?;
    fs::write(root.join("pins.rs"), source).ok()?;
    Some((root.clone(), FreshnessContext::new(root, false, false)))
}

fn check_source(source: &[u8]) -> (Option<String>, Vec<String>) {
    let Some((root, mut context)) = source_fixture(source) else {
        return (None, vec!["fixture creation failed".to_owned()]);
    };
    let value = context.rust_const("pins.rs", "PIN");
    let failures = context.failures.clone();
    let mut failures = failures;
    if let Err(error) = fs::remove_dir_all(&root) {
        failures.push(format!(
            "fixture cleanup failed for {}: {error}",
            root.display()
        ));
    }
    (value, failures)
}

#[test]
fn literal_authority_accepts_only_one_unconditional_public_str_constant() {
    let source = b"pub const PIN: &str = \"v1.2.3\";\n";
    assert_eq!(
        check_source(source),
        (Some("v1.2.3".to_owned()), Vec::new())
    );
    assert!(check_source(b"const PIN: &str = \"private\";").0.is_none());
    assert!(
        check_source(b"pub const PIN: &String = \"wrong\";")
            .0
            .is_none()
    );
    assert!(
        check_source(b"pub const PIN: &str = concat!(\"v\", \"1\");")
            .0
            .is_none()
    );
    assert!(
        check_source(b"pub mod nested { pub const PIN: &str = \"hidden\"; }")
            .0
            .is_none()
    );
    assert!(
        check_source(b"impl Thing { pub const PIN: &str = \"associated\"; }")
            .0
            .is_none()
    );
}

#[test]
fn literal_authority_ignores_comments_and_nested_token_scopes() {
    let source = br####"
// pub const PIN: &str = "line comment decoy";
/* nested /* pub const PIN: &str = "block comment decoy"; */ comment */
const NOTE: &str = "pub const PIN: &str = \"string decoy\";";
const RAW: &str = r###"pub const PIN: &str = "raw decoy";"###;
macro_rules! fake { () => { pub const PIN: &str = "macro decoy"; } }
mod nested { pub const PIN: &str = "nested decoy"; }
struct Holder;
impl Holder { pub const PIN: &str = "associated decoy"; }
/// Documentation comments do not change the compiled pin.
pub const PIN: &str = "v1.2.3";
"####;
    assert_eq!(check_source(source).0, Some("v1.2.3".to_owned()));
}

#[test]
fn conditional_crate_attrs_duplicates_and_constant_attrs_fail_closed() {
    for source in [
        b"#![cfg(feature = \"alternate\")] pub const PIN: &str = \"v1\";".as_slice(),
        b"#![cfg_attr(feature = \"alternate\", cfg(test))] pub const PIN: &str = \"v1\";",
        b"pub const PIN: &str = \"one\"; pub const PIN: &str = \"two\";",
        b"#[cfg(feature = \"alternate\")] pub const PIN: &str = \"v1\";",
    ] {
        assert!(
            check_source(source).0.is_none(),
            "source accepted: {}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn conditional_and_duplicate_pin_authorities_fail_closed() {
    for source in [
        b"#[cfg_attr(feature = \"alternate\", cfg(test))] pub const PIN: &str = \"v1\";".as_slice(),
        b"pub const PIN: &str = \"one\"; pub const PIN: &str = \"two\";",
        b"macro_rules! fake { () => { pub const PIN: &str = \"macro\"; } }",
        b"pub const PIN: &str = concat!(\"v\", \"1\");",
        b"pub const PIN: &String = \"wrong type\";",
        b"const PIN: &str = \"private\";",
        b"#[deprecated] pub const PIN: &str = \"attribute\";",
    ] {
        let (pin, failures) = check_source(source);
        assert!(
            pin.is_none() && !failures.is_empty(),
            "source accepted: {}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn escaped_and_raw_string_pins_decode_to_compiled_values() {
    for source in [
        b"pub const PIN: &str = \"1\\x2e2.3\";".as_slice(),
        b"pub const PIN: &str = r#\"1.2.3\"#;",
    ] {
        let (pin, failures) = check_source(source);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(pin, Some("1.2.3".to_owned()));
    }
    let (pin, failures) = check_source(b"pub const PIN: &str = concat!(\"1\", \".2.3\");");
    assert!(pin.is_none());
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("must use one string literal"))
    );
}

#[test]
fn literal_parser_enforces_utf8_and_source_cap() {
    assert!(check_source(b"\xff").1[0].contains("unsupported Rust source"));
    let prefix = b"pub const PIN: &str = \"v1\";";
    let mut exact = prefix.to_vec();
    exact.resize(RUST_SOURCE_CAP, b' ');
    assert_eq!(check_source(&exact).0, Some("v1".to_owned()));
    exact.push(b' ');
    assert!(check_source(&exact).1[0].contains("source exceeds"));
}

#[test]
fn strict_dates_and_timezone_timestamps_are_normalized() {
    assert!(parse_iso_date("2026-02-29").is_none());
    assert!(parse_iso_date("+026-10-05").is_none());
    assert!(parse_iso_date("2024-02-29").is_some());
    assert_eq!(
        parse_timestamp("2026-10-05T12:00:00+02:00"),
        parse_timestamp("2026-10-05T10:00:00Z")
    );
    assert_eq!(
        parse_timestamp("2026-10-05T10:00:00.1234Z"),
        parse_timestamp("2026-10-05T10:00:00z")
    );
    assert_eq!(
        parse_timestamp("2026-10-05T10:00:00-0130"),
        parse_timestamp("2026-10-05T11:30:00Z")
    );
    assert!(parse_timestamp("2026-10-05T99:00:00Z").is_none());
    for invalid in [
        "2026-10-05T-1:00:00Z",
        "2026-10-05T00:00:00.garbageZ",
        "2026-10-05T00:00:00.Z",
        "2026-10-05T00:00:00.1.2Z",
        "2026-10-05T00:00:00+01:0x",
        "2026-10-05T00:00:00+24:00",
        "2026-10-05T00:00:00",
        " 2026-10-05T00:00:00Z",
        "2026-10-05T00:00:00.garbageZ",
        "2026-10-05T-9223372036854775808:00:00Z",
        "2026-10-05T00:00:00-9223372036854775808:00:00Z",
    ] {
        assert!(parse_timestamp(invalid).is_none(), "accepted {invalid:?}");
    }
}
