//! P12 set-completeness, evidence-clock, and exception-model cases.
//!
//! Overflow from `p12_policy`: every case runs the real
//! `scripts/check-freshness.sh --root` against a fixture tree, so a passing
//! comment or doc string can never satisfy these tests.

use std::error::Error;

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";

/// Blessed standing record for `asamarts/alint` with `tag`.
fn blessed(tag: &str) -> String {
    format!(
        "{{\"key\":\"asamarts/alint\",\"kind\":\"mutable-tag\",\
         \"expiry_policy\":\"re-bless on pin moves\",\
         \"blessed_by\":\"version-policy §2/§4\",\"tag\":\"{tag}\",\
         \"expires\":null}}"
    )
}

/// Dated `exceptions` entry with full hold-style attribution for `key`.
fn dated(key: &str, granted: &str, expires: &str) -> String {
    format!(
        "{{\"key\":\"{key}\",\"held_version\":\"9.9.9\",\"owner\":\"team\",\
         \"issue\":\"#1\",\"reason\":\"blocked\",\"granted\":\"{granted}\",\
         \"expires\":\"{expires}\"}}"
    )
}

#[test]
fn missing_tool_row_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-missing-tool")?;
    let row = "{\"name\":\"release-plz\",\"pinned\":\"0.3.169\",\
        \"qualified\":\"0.3.169\",\
        \"source\":\"https://crates.io/api/v1/crates/release-plz\",\
        \"status\":\"current\"}";
    harness::mutate(&fixture.dir, INVENTORY, &format!(",{row}"), "")?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "tool release-plz");
    harness::assert_fail(&run, "inventory row missing");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn extra_tool_row_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-extra-tool")?;
    let row = ",{\"name\":\"evil-tool\",\"pinned\":\"1.0.0\",\
        \"qualified\":\"1.0.0\",\"source\":\"https://example.invalid/evil\",\
        \"status\":\"current\"}";
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "}],\"actions\"",
        &format!("}}{row}{tail}", tail = "],\"actions\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "outside the expected tool set");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn future_evidence_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-future-evidence")?;
    let today = harness::days_iso(0)?;
    let tomorrow = harness::days_iso(1)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"checked_at\":\"{today}\""),
        &format!("\"checked_at\":\"{tomorrow}\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "is in the future");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn future_clock_timestamp_inside_skew_grace_passes() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-future-clock-grace")?;
    let today = harness::days_iso(0)?;
    let future = harness::timestamp_iso(5 * 60)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"checked_at\":\"{today}\""),
        &format!("\"checked_at\":\"{future}\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn future_clock_timestamp_outside_skew_grace_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-future-clock-outside-grace")?;
    let today = harness::days_iso(0)?;
    let future = harness::timestamp_iso(7 * 60)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"checked_at\":\"{today}\""),
        &format!("\"checked_at\":\"{future}\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "is in the future");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn blessed_standing_exception_passes() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-blessed")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        &format!("\"exceptions\":[{}]", blessed("v0.16.1")),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    assert!(run.stdout.contains("blessed mutable tag"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn blessed_standing_wrong_tag_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-blessed-tag")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        &format!("\"exceptions\":[{}]", blessed("v9.9.9")),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "re-bless on pin moves");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn blessed_standing_missing_attribution_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-blessed-attr")?;
    let partial = "{\"key\":\"asamarts/alint\",\"kind\":\"mutable-tag\",\
        \"tag\":\"v0.16.1\",\"expires\":null}";
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        &format!("\"exceptions\":[{partial}]"),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "lacks expiry_policy,blessed_by");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn dated_exception_passes_with_full_attribution() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-dated")?;
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(5)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        &format!("\"exceptions\":[{}]", dated("gh", &granted, &expires)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn dated_exception_missing_field_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-dated-bad")?;
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(5)?;
    let bad = format!(
        "{{\"key\":\"gh\",\"held_version\":\"9.9.9\",\"issue\":\"#1\",\
         \"reason\":\"blocked\",\"granted\":\"{granted}\",\"expires\":\"{expires}\"}}"
    );
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        &format!("\"exceptions\":[{bad}]"),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "missing owner");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn non_object_entries_fail() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-shapes")?;
    let row = "{\"name\":\"mise\",\"pinned\":\"2026.9.16\",\
        \"qualified\":\"2026.9.16\",\
        \"source\":\"https://api.github.com/repos/jdx/mise/releases/latest\",\
        \"status\":\"current\"}";
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"tools\":[{row}"),
        "\"tools\":[42",
    )?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        "\"temporary_holds\":[42]",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "must be an object");
    harness::cleanup(&fixture);
    Ok(())
}
