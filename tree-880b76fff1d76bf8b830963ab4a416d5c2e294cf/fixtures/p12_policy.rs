//! P12 pin/mirror/exception/evidence cases against fixture trees.

use std::error::Error;

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";
const POLICY: &str = ".velnor/version-policy.toml";
const CATALOG: &str = "crates/adapters/velnor-actions-mise/src/catalog.rs";
const ACTIONS_RS: &str = "crates/adapters/velnor-actions-actionlint/src/actions.rs";
const MUTANTS: &str = ".cargo/mutants.toml";

/// Temporary-hold object with full attribution for `key`.
fn hold(key: &str, granted: &str, expires: &str) -> String {
    format!(
        "{{\"key\":\"{key}\",\"held_version\":\"9.9.9\",\"owner\":\"team\",\
         \"issue\":\"#1\",\"reason\":\"blocked\",\"granted\":\"{granted}\",\
         \"expires\":\"{expires}\"}}"
    )
}

#[test]
fn missing_action_row_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-missing-action")?;
    let row = "{\"key\":\"actions/cache/save\",\"pinned_version\":\"v6.1.0\",\
        \"pinned_sha\":\"55cc8345863c7cc4c66a329aec7e433d2d1c52a9\",\
        \"qualified_version\":\"v6.1.0\",\
        \"qualified_sha\":\"55cc8345863c7cc4c66a329aec7e433d2d1c52a9\",\
        \"source\":\"https://api.github.com/repos/actions/cache/releases/latest\",\
        \"status\":\"current\"}";
    harness::mutate(&fixture.dir, INVENTORY, &format!(",{row}"), "")?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "action actions/cache/save");
    harness::assert_fail(&run, "inventory row missing");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn local_pin_mismatch_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pin")?;
    harness::mutate(
        &fixture.dir,
        CATALOG,
        "RUST_VERSION: &str = \"1.98.1\"",
        "RUST_VERSION: &str = \"1.99.0\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "local-pin");
    harness::assert_fail(&run, "code=");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn policy_mirror_drift_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-mirror")?;
    harness::mutate(
        &fixture.dir,
        POLICY,
        "rust = \"1.98.1\"",
        "rust = \"1.99.0\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "policy-mirror");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn mutant_pin_drift_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-mutant-pin")?;
    harness::mutate(
        &fixture.dir,
        MUTANTS,
        "cargo-mutants = \"27.1.0\"",
        "cargo-mutants = \"27.2.0\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "validation-tools/cargo-mutants");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn dangling_mutant_glob_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-mutant-glob")?;
    harness::mutate(
        &fixture.dir,
        MUTANTS,
        "\"crates/aaa/Cargo.toml\"",
        "\"crates/aaa/Nope.rs\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "matches no production file");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn stale_recorded_latest_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-latest")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"name\":\"gh\",\"pinned\":\"2.101.0\"",
        "\"name\":\"gh\",\"pinned\":\"2.101.0\",\"latest\":\"v9.9.9\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "stale pin");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn stale_evidence_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-evidence")?;
    let today = harness::days_iso(0)?;
    let old = harness::days_iso(-30)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"checked_at\":\"{today}\""),
        &format!("\"checked_at\":\"{old}\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "stale evidence");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unknown_status_is_never_current() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-unknown")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"source\":\"https://api.github.com/repos/cli/cli/releases/latest\",\"status\":\"current\"",
        "\"source\":\"https://api.github.com/repos/cli/cli/releases/latest\",\"status\":\"unknown\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "never current");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn held_status_needs_a_covering_hold() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-held")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"source\":\"https://api.github.com/repos/cli/cli/releases/latest\",\"status\":\"current\"",
        "\"source\":\"https://api.github.com/repos/cli/cli/releases/latest\",\"status\":\"held\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "without a covering temporary hold");
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(5)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{}]", hold("gh", &granted, &expires)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn malformed_exception_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-bad")?;
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(5)?;
    let bad = format!(
        "{{\"key\":\"gh\",\"held_version\":\"9.9.9\",\"owner\":\"team\",\
         \"reason\":\"blocked\",\"granted\":\"{granted}\",\"expires\":\"{expires}\"}}"
    );
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{bad}]"),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "missing issue");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn expired_exception_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-expired")?;
    let granted = harness::days_iso(-10)?;
    let expires = harness::days_iso(-1)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{}]", hold("gh", &granted, &expires)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "expired");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn future_granted_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-future")?;
    let granted = harness::days_iso(1)?;
    let expires = harness::days_iso(5)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{}]", hold("gh", &granted, &expires)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "in the future");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn inverted_window_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-inverted")?;
    let today = harness::days_iso(0)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{}]", hold("gh", &today, &today)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "inverted window");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn overlong_exception_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-long")?;
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(20)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!("\"temporary_holds\":[{}]", hold("gh", &granted, &expires)),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "exceeds max 14d");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unblessed_standing_exception_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-standing")?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"exceptions\":[]",
        "\"exceptions\":[{\"key\":\"gh\",\"kind\":\"handwave\",\"expires\":null}]",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "without a spec blessing");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unknown_hold_subject_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-hold-subject")?;
    let granted = harness::days_iso(-1)?;
    let expires = harness::days_iso(5)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        "\"temporary_holds\":[]",
        &format!(
            "\"temporary_holds\":[{}]",
            hold("no-such-thing", &granted, &expires)
        ),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "matches no inventoried");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn action_const_wiring_is_mapped() -> Result<(), Box<dyn Error>> {
    let script = crate::impl_repo_policy::read("scripts/check-freshness.sh")?;
    assert!(
        script.contains(ACTIONS_RS),
        "action const path must be read"
    );
    for key in [
        "actions/cache/restore",
        "actions/cache/save",
        "asamarts/alint",
        "Swatinem/rust-cache",
    ] {
        assert!(script.contains(key), "expected action set misses {key}");
    }
    Ok(())
}
