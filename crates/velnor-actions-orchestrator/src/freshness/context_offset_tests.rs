use super::context::{FreshnessContext, RUST_SOURCE_CAP, parse_timestamp};
use serde_json::json;
use std::fs;

#[test]
fn malformed_four_byte_non_ascii_offset_fails_without_panicking() {
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+€x")).is_none());
}

#[test]
fn numeric_offsets_and_date_only_timestamps_remain_valid() {
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+01:30")).is_some());
    assert!(parse_timestamp(&json!("2026-10-05T12:34:56+0130")).is_some());
    assert!(parse_timestamp(&json!("2026-10-05")).is_some());
}

#[test]
fn rust_source_read_rejects_content_over_the_cap() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("pins.rs"), vec![b' '; RUST_SOURCE_CAP + 1])?;
    let mut context = FreshnessContext::new(root.path(), false, false);

    assert!(context.rust_const("pins.rs", "PIN").is_none());
    assert!(
        context
            .failures
            .iter()
            .any(|failure| failure.contains("source exceeds"))
    );
    Ok(())
}

#[test]
fn rust_source_read_accepts_content_at_the_cap() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let declaration = b"pub const PIN: &str = \"1.2.3\";\n";
    let mut source = declaration.to_vec();
    source.resize(RUST_SOURCE_CAP, b' ');
    fs::write(root.path().join("pins.rs"), source)?;
    let mut context = FreshnessContext::new(root.path(), false, false);

    assert_eq!(
        context.rust_const("pins.rs", "PIN").as_deref(),
        Some("1.2.3")
    );
    assert!(context.failures.is_empty());
    Ok(())
}

#[test]
fn rust_source_read_rejects_invalid_utf8() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("pins.rs"), [0xff])?;
    let mut context = FreshnessContext::new(root.path(), false, false);

    assert!(context.rust_const("pins.rs", "PIN").is_none());
    assert!(
        context
            .failures
            .iter()
            .any(|failure| failure.contains("unsupported Rust source"))
    );
    Ok(())
}
