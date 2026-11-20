// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn with_restored(restored: Option<bool>) -> LaneMetrics {
    let mut value = lane();
    if let Some(restored) = restored {
        value["invocations"][0]["compilations"][0]["restored"] = json!(restored);
    }
    metrics(value)
}

#[test]
fn a_dependency_unit_the_restored_base_held_is_still_waste() {
    assert_eq!(
        waste(&with_restored(Some(true))),
        [".: serde v1.0.0 compiled despite usable restored inputs"]
    );
}

#[test]
fn a_dependency_unit_the_restored_base_never_held_is_a_notice_not_waste() {
    let lane = with_restored(Some(false));
    assert_eq!(waste(&lane), [] as [String; 0]);
    let report = check(&[lane]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(report.contains("no waste"), "{report}");
    assert!(
        report.contains(
            ".: dependency packages not in the restored base (a different package or feature selection than the run that saved it): serde v1.0.0"
        ),
        "{report}"
    );
}

#[test]
fn a_run_without_a_restored_unit_record_keeps_the_old_rule() {
    let lane = with_restored(None);
    assert_eq!(
        waste(&lane),
        [".: serde v1.0.0 compiled despite usable restored inputs"]
    );
    assert!(check(&[lane]).is_err());
}

#[test]
fn a_gap_is_no_notice_on_a_cold_base() {
    let mut value = lane();
    value["caches"][0]["usable"] = json!(false);
    value["caches"][0]["usable_same_lock"] = json!(false);
    value["invocations"][0]["restored_target_usable"] = json!(false);
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    let report = check(&[metrics(value)]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(!report.contains("restored base"), "{report}");
}

#[test]
fn a_record_that_is_not_ours_or_is_damaged_reads_as_absent() {
    use std::collections::BTreeSet;
    use velnor_ci::restored_units::{decode, encode};
    let hashes: BTreeSet<String> = ["0123456789abcdef", "fedcba9876543210"]
        .map(str::to_owned)
        .into();
    assert_eq!(decode(&encode(&hashes)), Some(hashes));
    assert_eq!(decode(&encode(&BTreeSet::new())), Some(BTreeSet::new()));
    for text in [
        "",
        "other 1\n0123456789abcdef\n",
        "velnor-restored-units 1\nnot-a-hash\n",
    ] {
        assert_eq!(decode(text), None, "{text:?}");
    }
}

#[test]
fn a_gap_on_a_changed_lock_is_no_notice() {
    let mut value = lane();
    value["caches"][0]["usable_same_lock"] = json!(false);
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    let report = check(&[metrics(value)]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(!report.contains("restored base"), "{report}");
}
