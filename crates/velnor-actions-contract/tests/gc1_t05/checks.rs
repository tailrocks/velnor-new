// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::canonical::{canonical_json_bytes, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    EffectiveJobBindingV1, ExecutionBranch, ExecutionContextV1, JobBindingV1, MeasuredU64,
    UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::workflow::report::TaskStatus;

use super::fixtures::{
    digest_fill, fixture_context, fixture_effective, fixture_job, fixture_ledger, fixture_report,
    fixture_source_binding, fixture_unit, fixture_unknown,
};

pub(super) fn rejects_bad_job() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_job()?;
    bad.schema = 2;
    assert!(bad.validate().is_err());
    let mut bad = fixture_job()?;
    bad.repository_id = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_job()?;
    bad.run_attempt = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_job()?;
    bad.job_id = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_job()?;
    "not-a-sha".clone_into(&mut bad.workflow_sha);
    assert!(bad.validate().is_err());
    let mut bad = fixture_job()?;
    bad.job_key.clear();
    assert!(bad.validate().is_err());
    assert!(JobBindingV1::parse_json("not json").is_err());
    Ok(())
}

pub(super) fn rejects_bad_context() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_context()?;
    "relative/path".clone_into(&mut bad.workspace_root);
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.transport_root.clone_from(&bad.workspace_root);
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.lanes.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.lanes.push(bad.lanes[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    "/absolute".clone_into(&mut bad.lanes[0].target_dir);
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.mbx_target_views = true;
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.compiler_adapters.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.compiler_adapters.push(bad.compiler_adapters[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    "relative/adapter".clone_into(&mut bad.compiler_adapters[0].stable_path);
    assert!(bad.validate().is_err());
    let text = String::from_utf8(fixture_context()?.to_canonical_bytes()?)?;
    let tampered = text.replacen("\"schema\":1", "\"schema\":1,\"surprise\":true", 1);
    assert!(ExecutionContextV1::parse_json(&tampered).is_err());
    let mut bad = fixture_source_binding()?;
    "short".clone_into(&mut bad.admitted_source_sha);
    assert!(bad.validate().is_err());
    let mut bad = fixture_source_binding()?;
    bad.admitted_tree_id.clear();
    assert!(bad.validate().is_err());
    Ok(())
}

pub(super) fn rejects_bad_units() -> Result<(), Box<dyn Error>> {
    assert!(fixture_unit("unit-a", UnitOutcome::Executed, None).is_err());
    assert!(fixture_unit("unit-a", UnitOutcome::Failed, None).is_err());
    assert!(fixture_unit("unit-a", UnitOutcome::CacheHit, Some(0)).is_err());
    assert!(fixture_unit("unit-a", UnitOutcome::NeedsPrepare, Some(0)).is_err());
    assert!(fixture_unit("unit-a", UnitOutcome::Cancelled, Some(0)).is_err());
    let mut bad = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    "26-10-2026".clone_into(&mut bad.started_at);
    assert!(bad.validate().is_err());
    let mut bad = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    "2026-10-26T00:00:00".clone_into(&mut bad.finished_at);
    assert!(bad.validate().is_err());
    let mut bad = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    "2026-10-25T00:00:00Z".clone_into(&mut bad.finished_at);
    assert!(bad.validate().is_err());
    let mut bad = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    bad.unit_id.clear();
    assert!(bad.validate().is_err());
    assert!(UnitResultV1::parse_json("[]").is_err());
    Ok(())
}

pub(super) fn rejects_bad_ledger() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_ledger()?;
    bad.selected_unit_ids.clear();
    bad.ordered_results.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_ledger()?;
    bad.selected_unit_ids = vec!["unit-b".to_owned(), "unit-a".to_owned()];
    assert!(bad.validate().is_err());
    let mut bad = fixture_ledger()?;
    bad.ordered_results.pop();
    assert!(bad.validate().is_err());
    let mut bad = fixture_ledger()?;
    bad.ordered_results.swap(0, 1);
    assert!(bad.validate().is_err());
    let mut bad = fixture_ledger()?;
    bad.selected_unit_ids = vec!["unit-a".to_owned(), "unit-a".to_owned()];
    assert!(bad.validate().is_err());
    let mut bad = fixture_ledger()?;
    bad.execution_branch = ExecutionBranch::Writer;
    bad.validate()?;
    let text = String::from_utf8(fixture_ledger()?.to_canonical_bytes()?)?;
    assert!(UnitLedgerV1::parse_json("not json").is_err());
    let duplicated = text.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    assert!(UnitLedgerV1::parse_json(&duplicated).is_err());
    Ok(())
}

pub(super) fn rejects_bad_effective_and_measured() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_effective()?;
    bad.job_id = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_effective()?;
    bad.actual_attempt = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_effective()?;
    bad.job_key.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_effective()?;
    "xyz".clone_into(&mut bad.source_sha);
    assert!(bad.validate().is_err());
    assert!(EffectiveJobBindingV1::parse_json("not json").is_err());

    let both = MeasuredU64 {
        value: Some(3),
        unknown_reason: fixture_unknown()?.unknown_reason,
    };
    assert!(both.validate().is_err());
    let neither = MeasuredU64 {
        value: None,
        unknown_reason: None,
    };
    assert!(neither.validate().is_err());
    let mut bad_gap = fixture_unknown()?;
    if let Some(reason) = bad_gap.unknown_reason.as_mut() {
        reason.detail.clear();
    }
    assert!(bad_gap.validate().is_err());
    assert!(MeasuredU64::parse_json("not json").is_err());
    Ok(())
}

pub(super) fn rejects_bad_report_binding() -> Result<(), Box<dyn Error>> {
    let report = fixture_report(TaskStatus::Executed, 0)?;
    let mut hit = fixture_unit("unit-b", UnitOutcome::CacheHit, None)?;
    hit.report_digest = digest_b3_typed(&canonical_json_bytes(&report)?);
    assert!(report.validate_bound_unit_result(&hit).is_err());

    let reused = fixture_report(TaskStatus::Reused, 0)?;
    let mut executed = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    executed.report_digest = digest_b3_typed(&canonical_json_bytes(&reused)?);
    assert!(reused.validate_bound_unit_result(&executed).is_err());

    let mut foreign = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    foreign.report_digest = digest_fill('e')?;
    assert!(report.validate_bound_unit_result(&foreign).is_err());

    let mut mismatch = fixture_unit("unit-a", UnitOutcome::Executed, Some(1))?;
    mismatch.report_digest = digest_b3_typed(&canonical_json_bytes(&report)?);
    assert!(report.validate_bound_unit_result(&mismatch).is_err());

    let failed_report = fixture_report(TaskStatus::Failed, 1)?;
    let mut failed = fixture_unit("unit-a", UnitOutcome::Failed, Some(1))?;
    failed.report_digest = digest_b3_typed(&canonical_json_bytes(&failed_report)?);
    failed_report.validate_bound_unit_result(&failed)?;

    let failed_report = fixture_report(TaskStatus::Failed, 0)?;
    let mut zero_exit = fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?;
    zero_exit.report_digest = digest_b3_typed(&canonical_json_bytes(&failed_report)?);
    assert!(
        failed_report
            .validate_bound_unit_result(&zero_exit)
            .is_err()
    );
    Ok(())
}
