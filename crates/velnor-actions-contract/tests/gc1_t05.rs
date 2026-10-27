// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checks;
mod fixtures;

use std::error::Error;

use velnor_actions_contract::canonical::{canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    CacheRestoreOutcome, EffectiveJobBindingV1, ExecutionContextV1, MeasuredU64, SourceBindingV1,
    UnitOutcome,
};
use velnor_actions_contract::workflow::report::TaskStatus;

use fixtures::{
    fixture_context, fixture_effective, fixture_ledger, fixture_report, fixture_source_binding,
    fixture_unit, fixture_unknown,
};

#[test]
fn gc1_t05_preserves_contract() -> Result<(), Box<dyn Error>> {
    let context = fixture_context()?;
    let bytes = context.to_canonical_bytes()?;
    let text = String::from_utf8(bytes.clone())?;
    let roundtrip = ExecutionContextV1::parse_json(&text)?;
    assert_eq!(context, roundtrip);
    let rebuilt = fixture_context()?;
    assert_eq!(context.context_digest()?, rebuilt.context_digest()?);
    let mut moved = fixture_context()?;
    moved.workspace_root = "/home/runner/work/other/other".to_owned();
    assert_ne!(context.context_digest()?, moved.context_digest()?);

    let binding = fixture_source_binding()?;
    let binding_bytes = binding.to_canonical_bytes()?;
    let binding_text = String::from_utf8(binding_bytes)?;
    assert_eq!(binding, SourceBindingV1::parse_json(&binding_text)?);

    let ledger = fixture_ledger()?;
    ledger.validate()?;
    assert_eq!(ledger.successful_completion_unit_ids(), vec!["unit-a"]);
    assert!(!fixture_unit("unit-b", UnitOutcome::CacheHit, None)?.is_successful_completion());
    assert!(fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?.is_successful_completion());
    assert!(!fixture_unit("unit-a", UnitOutcome::Executed, Some(1))?.is_successful_completion());
    assert_eq!(ledger.ledger_digest()?, fixture_ledger()?.ledger_digest()?);

    assert!(!CacheRestoreOutcome::Hit.requires_cold_execution());
    assert!(!CacheRestoreOutcome::Partial.requires_cold_execution());
    assert!(CacheRestoreOutcome::Cold.requires_cold_execution());
    assert!(CacheRestoreOutcome::Denied.requires_cold_execution());
    assert!(CacheRestoreOutcome::Corrupt.requires_cold_execution());
    assert!(CacheRestoreOutcome::Unavailable.requires_cold_execution());

    let effective = fixture_effective()?;
    effective.validate()?;
    assert!(effective.retained_covers(456, 1));
    assert!(!effective.retained_covers(456, 2));
    assert!(!effective.retained_covers(999, 1));
    let rerun = EffectiveJobBindingV1 {
        retained_without_rerun: false,
        ..fixture_effective()?
    };
    assert!(!rerun.retained_covers(456, 1));
    let effective_bytes = effective.to_canonical_bytes()?;
    let effective_text = String::from_utf8(effective_bytes)?;
    assert_eq!(
        effective,
        EffectiveJobBindingV1::parse_json(&effective_text)?
    );

    let observed = MeasuredU64 {
        value: Some(0),
        unknown_reason: None,
    };
    observed.validate()?;
    assert_eq!(
        observed,
        MeasuredU64::parse_json(&String::from_utf8(observed.to_canonical_bytes()?)?)?
    );
    let unknown = fixture_unknown()?;
    unknown.validate()?;
    let unknown_text = String::from_utf8(unknown.to_canonical_bytes()?)?;
    assert_eq!(unknown, MeasuredU64::parse_json(&unknown_text)?);
    assert!(unknown_text.contains("\"value\":null"));

    let report = fixture_report(TaskStatus::Executed, 0)?;
    report.validate()?;
    assert!(report.is_successful_completion());
    let reused = fixture_report(TaskStatus::Reused, 0)?;
    assert!(!reused.is_successful_completion());
    let mut executed = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    executed.report_digest = digest_b3_typed(&canonical_json_bytes(&report)?);
    report.validate_bound_unit_result(&executed)?;

    let mut hit = fixture_unit("unit-b", UnitOutcome::CacheHit, None)?;
    hit.report_digest = digest_b3_typed(&canonical_json_bytes(&reused)?);
    reused.validate_bound_unit_result(&hit)?;
    Ok(())
}

#[test]
fn gc1_t05_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    checks::rejects_bad_job()?;
    checks::rejects_bad_context()?;
    checks::rejects_bad_units()?;
    checks::rejects_bad_ledger()?;
    checks::rejects_bad_effective_and_measured()?;
    checks::rejects_bad_report_binding()?;
    Ok(())
}
