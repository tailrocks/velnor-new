// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;

const SAVES: &[&str] = &[
    "velnor-ci inputs save",
    "Decide whether to save the build directory",
    "Save the target cache",
    "Record the target cache key",
];

const AUDIT: &str = "velnor-ci inputs audit --root .";

fn test_lane(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs(helper, "shape = \"lint-test\"\n")?
        .into_iter()
        .find(|(id, _)| id == "rust-test")
        .map(|(_, lane)| lane)
        .ok_or_else(|| "no rust-test job".into())
}

fn position(order: &[String], name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    order
        .iter()
        .position(|candidate| candidate == name)
        .ok_or_else(|| format!("no step {name}").into())
}

#[test]
fn the_split_test_lane_audits_the_build_before_it_prunes_and_saves() -> TestResult {
    let lane = test_lane("0.1.116")?;
    let order = names(&lane);
    let audit = position(&order, AUDIT)?;
    for save in SAVES {
        assert!(audit < position(&order, save)?, "{save}: {order:?}");
    }
    Ok(())
}

#[test]
fn the_split_test_lane_saves_only_after_the_audit_passed() -> TestResult {
    let lane = test_lane("0.1.116")?;
    let saved = step(&lane, "velnor-ci inputs save").ok_or("missing save")?;
    assert!(
        saved.contains("steps.audit_1.outcome == ''success''"),
        "{saved}"
    );
    Ok(())
}

#[test]
fn a_helper_below_0_1_116_keeps_the_audit_after_the_saves_in_the_split_test_lane() -> TestResult {
    let lane = test_lane("0.1.115")?;
    let order = names(&lane);
    let audit = position(&order, AUDIT)?;
    assert!(
        position(&order, "Save the target cache")? < audit,
        "{order:?}"
    );
    Ok(())
}
