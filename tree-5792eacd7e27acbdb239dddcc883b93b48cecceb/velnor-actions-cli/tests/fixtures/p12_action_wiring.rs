//! Action pin constant wiring against the real freshness-gate wrapper.

use std::error::Error;

use super::p12_harness as harness;

const ACTIONS_RS: &str = "crates/velnor-actions-actionlint/src/actions.rs";

struct ActionPinCase {
    keys: &'static [&'static str],
    prefix: &'static str,
    version: &'static str,
    sha: &'static str,
}

const CASES: [ActionPinCase; 8] = [
    ActionPinCase {
        keys: &["jdx/mise-action"],
        prefix: "MISE_ACTION",
        version: "v4.3.0",
        sha: "c2a87611a18de5b3828c5652fe268e992400cb5c",
    },
    ActionPinCase {
        keys: &["actions/checkout"],
        prefix: "CHECKOUT_ACTION",
        version: "v7.0.1",
        sha: "3d3c42e5aac5ba805825da76410c181273ba90b1",
    },
    ActionPinCase {
        keys: &["actions/download-artifact"],
        prefix: "DOWNLOAD_ARTIFACT_ACTION",
        version: "v8.0.1",
        sha: "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
    },
    ActionPinCase {
        keys: &["actions/upload-artifact"],
        prefix: "UPLOAD_ARTIFACT_ACTION",
        version: "v7.0.1",
        sha: "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    },
    ActionPinCase {
        keys: &["actions/cache/restore", "actions/cache/save"],
        prefix: "CACHE_ACTION",
        version: "v6.1.0",
        sha: "55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    },
    ActionPinCase {
        keys: &["jdx/mr-boxington-action"],
        prefix: "MR_BOXINGTON_ACTION",
        version: "v1.5.0",
        sha: "9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3",
    },
    ActionPinCase {
        keys: &["asamarts/alint"],
        prefix: "ALINT_ACTION",
        version: "v0.16.1",
        sha: "9f9d34ba0eae3888299b9e570f43338b0e7f2cdb",
    },
    ActionPinCase {
        keys: &["Swatinem/rust-cache"],
        prefix: "RUST_CACHE_ACTION",
        version: "v2.9.2",
        sha: "6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
    },
];

#[test]
fn action_const_wiring_is_mapped() -> Result<(), Box<dyn Error>> {
    for (index, case) in CASES.iter().enumerate() {
        exercise_case(index, case)?;
    }
    Ok(())
}

fn exercise_case(index: usize, case: &ActionPinCase) -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing(&format!("p12-action-const-{index}"))?;
    assert_clean_mapping(&fixture, case)?;
    check_version_drift(&fixture, case)?;
    check_sha_drift(&fixture, case)?;
    harness::cleanup(&fixture);
    Ok(())
}

fn assert_clean_mapping(
    fixture: &harness::Fixture,
    case: &ActionPinCase,
) -> Result<(), Box<dyn Error>> {
    let baseline = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&baseline);
    for key in case.keys {
        assert_mapped_pin(&baseline, key, case.prefix, "version", case.version);
        assert_mapped_pin(&baseline, key, case.prefix, "sha", case.sha);
    }
    Ok(())
}

fn assert_mapped_pin(run: &harness::Run, key: &str, prefix: &str, field: &str, value: &str) {
    let suffix = if field == "version" { "VERSION" } else { "SHA" };
    let subject = format!("action {key} {field} ({ACTIONS_RS}::{prefix}_{suffix})");
    assert!(
        harness::rows(run).iter().any(|row| {
            row.contains("\"check\":\"local-pin\"")
                && row.contains("\"status\":\"pass\"")
                && row.contains(&format!("\"subject\":\"{subject}\""))
                && row.contains(&format!("\"detail\":\"{value}\""))
        }),
        "missing clean local-pin mapping for {subject}={value}:\n{}",
        run.stdout
    );
}

fn check_version_drift(
    fixture: &harness::Fixture,
    case: &ActionPinCase,
) -> Result<(), Box<dyn Error>> {
    let drift = format!("{}-drift", case.version);
    mutate_pin(fixture, case, "VERSION", case.version, &drift)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    assert_pin_drift(&run, case, "version", "VERSION", &drift, case.version);
    mutate_pin(fixture, case, "VERSION", &drift, case.version)?;
    harness::assert_clean(&harness::run_script(&fixture.dir, &[])?);
    Ok(())
}

fn check_sha_drift(fixture: &harness::Fixture, case: &ActionPinCase) -> Result<(), Box<dyn Error>> {
    let drift = "0".repeat(40);
    mutate_pin(fixture, case, "SHA", case.sha, &drift)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    assert_pin_drift(&run, case, "sha", "SHA", &drift, case.sha);
    mutate_pin(fixture, case, "SHA", &drift, case.sha)?;
    harness::assert_clean(&harness::run_script(&fixture.dir, &[])?);
    Ok(())
}

fn mutate_pin(
    fixture: &harness::Fixture,
    case: &ActionPinCase,
    suffix: &str,
    old: &str,
    new: &str,
) -> Result<(), Box<dyn Error>> {
    let old_decl = format!("pub const {}_{suffix}: &str = \"{old}\";", case.prefix);
    let new_decl = format!("pub const {}_{suffix}: &str = \"{new}\";", case.prefix);
    harness::mutate(&fixture.dir, ACTIONS_RS, &old_decl, &new_decl)
}

fn assert_pin_drift(
    run: &harness::Run,
    case: &ActionPinCase,
    field: &str,
    suffix: &str,
    code: &str,
    inventory: &str,
) {
    for key in case.keys {
        assert_action_pin_row(run, key, case.prefix, field, suffix, code, inventory);
    }
}

fn assert_action_pin_row(
    run: &harness::Run,
    key: &str,
    prefix: &str,
    field: &str,
    suffix: &str,
    code: &str,
    inventory: &str,
) {
    let subject = format!("action {key} {field} ({ACTIONS_RS}::{prefix}_{suffix})");
    let detail = format!("code='{code}' inventory='{inventory}'");
    assert_ne!(run.code, 0, "expected checker failure for {subject}");
    assert!(
        harness::rows(run).iter().any(|row| {
            row.contains("\"check\":\"local-pin\"")
                && row.contains("\"status\":\"fail\"")
                && row.contains(&format!("\"subject\":\"{subject}\""))
                && row.contains(&format!("\"detail\":\"{detail}\""))
        }),
        "missing exact local-pin failure for {subject}: {detail}\n{}",
        run.stdout
    );
}
