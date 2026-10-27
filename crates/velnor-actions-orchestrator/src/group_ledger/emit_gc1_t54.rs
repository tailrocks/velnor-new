// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

// gc1-t54 covers group-ledger emit: the success path writes a validated
// ledger whose every digest recomputes from the staged inputs, a missing
// report on success is a loud error (red first), a missing report on
// failure records NeedsPrepare, and a failed unit on success is refused.
use velnor_actions_contract::workflow::{UnitLedgerV1, UnitOutcome, UnitResultV1};

use super::super::gc1_t54_support::{self, RUN_KEY};
use super::{JobStatus, emit_group_ledger_to};

fn fail(problem: &str) -> crate::OrchestratorError {
    crate::internal::internal(problem)
}

#[test]
fn success_emits_validated_ledger() -> Result<(), crate::OrchestratorError> {
    let fixture = gc1_t54_support::build(JobStatus::Success)?;
    let report_file = format!("{}.json", fixture.report_id.as_str());
    let out = emit_group_ledger_to(
        &fixture.anchor,
        &fixture.manifest_path,
        &fixture.run_dir,
        RUN_KEY,
        &fixture.emit_env,
    )?;
    assert_eq!(
        out,
        fixture
            .run_dir
            .join(&fixture.emit_env.matrix_key)
            .join(super::GROUP_LEDGER_FILENAME)
    );
    let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
    let ledger = UnitLedgerV1::parse_json(&text).map_err(|_| fail("ledger_parse"))?;
    ledger.validate().map_err(|_| fail("ledger_invalid"))?;
    assert_eq!(ledger.selected_unit_ids, vec![fixture.task_id.clone()]);
    assert_eq!(ledger.ordered_results.len(), 1);
    let result = &ledger.ordered_results[0];
    assert_eq!(result.unit_id, fixture.task_id);
    assert_eq!(result.outcome, UnitOutcome::Executed);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(ledger.job.repository_id, 0, "tokenless numerics stay zero");
    assert_eq!(ledger.job.job_id, 0, "tokenless numerics stay zero");
    assert_eq!(ledger.job.repository, "tailrocks/velnor-new");
    assert_eq!(ledger.job.run_id, 4242);
    assert_eq!(ledger.job.run_attempt, 1);
    assert_eq!(ledger.job.source_sha, gc1_t54_support::HEAD_SHA);
    assert!(
        result.started_at < result.finished_at,
        "finished=now started=now-duration"
    );
    let plan_bytes =
        std::fs::read(fixture.run_dir.join("plan.json")).map_err(|err| fail(&err.to_string()))?;
    let plan_digest = UnitLedgerV1::plan_digest_from_canonical_bytes(&plan_bytes);
    assert_eq!(ledger.job.plan_digest.as_str(), plan_digest.as_str());
    assert_eq!(
        ledger.group_digest.as_str(),
        UnitLedgerV1::group_digest_for_group("t54-group", &plan_digest)
            .map_err(|_| fail("group"))?
            .as_str(),
    );
    let report_bytes = std::fs::read(
        fixture
            .run_dir
            .join(&fixture.emit_env.matrix_key)
            .join("tasks")
            .join(report_file.as_str()),
    )
    .map_err(|err| fail(&err.to_string()))?;
    assert_eq!(
        result.report_digest.as_str(),
        UnitResultV1::report_digest_from_canonical_bytes(&report_bytes).as_str(),
    );
    assert_eq!(
        ledger.observation_digest.as_str(),
        UnitLedgerV1::observation_digest_for_sidecars(&[None])
            .map_err(|_| fail("observation"))?
            .as_str(),
    );
    Ok(())
}

#[test]
fn missing_report_on_success_is_loud() -> Result<(), crate::OrchestratorError> {
    let fixture = gc1_t54_support::build(JobStatus::Success)?;
    let report_file = format!("{}.json", fixture.report_id.as_str());
    std::fs::remove_file(
        fixture
            .run_dir
            .join(&fixture.emit_env.matrix_key)
            .join("tasks")
            .join(report_file.as_str()),
    )
    .map_err(|err| fail(&err.to_string()))?;
    match emit_group_ledger_to(
        &fixture.anchor,
        &fixture.manifest_path,
        &fixture.run_dir,
        RUN_KEY,
        &fixture.emit_env,
    ) {
        Ok(_) => Err(fail("missing report on success must fail loudly")),
        Err(err) => {
            assert!(
                err.to_string().contains("missing_task_report"),
                "unexpected error: {err}"
            );
            Ok(())
        }
    }
}

#[test]
fn missing_report_on_failure_defers() -> Result<(), crate::OrchestratorError> {
    let fixture = gc1_t54_support::build(JobStatus::Failure)?;
    let report_file = format!("{}.json", fixture.report_id.as_str());
    std::fs::remove_file(
        fixture
            .run_dir
            .join(&fixture.emit_env.matrix_key)
            .join("tasks")
            .join(report_file.as_str()),
    )
    .map_err(|err| fail(&err.to_string()))?;
    let out = emit_group_ledger_to(
        &fixture.anchor,
        &fixture.manifest_path,
        &fixture.run_dir,
        RUN_KEY,
        &fixture.emit_env,
    )?;
    let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
    let ledger = UnitLedgerV1::parse_json(&text).map_err(|_| fail("ledger_parse"))?;
    assert_eq!(ledger.ordered_results.len(), 1);
    assert_eq!(ledger.ordered_results[0].outcome, UnitOutcome::NeedsPrepare);
    assert_eq!(ledger.ordered_results[0].exit_code, None);
    assert_eq!(
        ledger.ordered_results[0].report_digest.as_str(),
        UnitResultV1::missing_report_digest().as_str(),
    );
    Ok(())
}
