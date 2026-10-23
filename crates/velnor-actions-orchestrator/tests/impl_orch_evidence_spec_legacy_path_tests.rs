//! Exact-path and out-of-scope identifier controls.

use std::fs;

use crate::impl_common::TestResult;

use super::{
    ACTUAL_SOURCE, assert_three_passive_spans, fixture, legacy_fetch_hits, mask, replace_once,
};

#[test]
fn wrong_paths_and_unresolved_paths_do_not_receive_the_exception() -> TestResult {
    let actual = fixture(ACTUAL_SOURCE)?;
    let alternate_root = fixture(ACTUAL_SOURCE)?;
    let unmasked = super::super::mask_resolved_evidence_spec_for_legacy_scan(
        &alternate_root.source_path,
        &actual.root,
        ACTUAL_SOURCE,
    )?;
    assert_eq!(unmasked, ACTUAL_SOURCE);
    assert!(legacy_fetch_hits(&unmasked));

    let elsewhere = actual.temp.path().join("other");
    fs::create_dir_all(&elsewhere)?;
    let same_basename = elsewhere.join("mise_task_suites_emit.rs");
    fs::write(&same_basename, ACTUAL_SOURCE)?;
    let unmasked = super::super::mask_resolved_evidence_spec_for_legacy_scan(
        &same_basename,
        &actual.root,
        ACTUAL_SOURCE,
    )?;
    assert_eq!(unmasked, ACTUAL_SOURCE);
    assert!(legacy_fetch_hits(&unmasked));

    let missing = actual.root.join("missing.rs");
    assert!(
        super::super::mask_resolved_evidence_spec_for_legacy_scan(
            &missing,
            &actual.root,
            ACTUAL_SOURCE,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn exact_path_does_not_turn_other_fetch_identifiers_into_allowlisted_data() -> TestResult {
    let source = replace_once(
        ACTUAL_SOURCE,
        "let resolved =",
        "let fetch_unapproved_inventory = None;\n        let resolved =",
    );
    let (_fixture, masked) = mask(&source)?;
    assert_three_passive_spans(&source, &masked);
    assert!(masked.contains("let fetch_unapproved_inventory = None;"));
    assert!(legacy_fetch_hits(&masked));
    Ok(())
}
