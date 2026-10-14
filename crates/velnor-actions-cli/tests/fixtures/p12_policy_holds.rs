//! Hold-subject and action-wiring freshness cases.

use std::error::Error;

use super::{ACTIONS_RS, INVENTORY, hold};
use crate::impl_repo_policy::p12_harness as harness;

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
    let inventory =
        crate::impl_repo_policy::read("crates/velnor-actions-freshness/src/inventory.rs")?;
    let pins =
        crate::impl_repo_policy::read("crates/velnor-actions-freshness/src/inventory/pins.rs")?;
    assert!(
        inventory.contains(ACTIONS_RS) && pins.contains("EXPECTED_ACTIONS"),
        "Rust freshness owner must map action consts"
    );
    for key in [
        "actions/cache/restore",
        "actions/cache/save",
        "asamarts/alint",
    ] {
        assert!(inventory.contains(key), "expected action set misses {key}");
    }
    assert!(
        !inventory.contains("Swatinem/rust-cache"),
        "retired action remains mapped"
    );
    Ok(())
}
