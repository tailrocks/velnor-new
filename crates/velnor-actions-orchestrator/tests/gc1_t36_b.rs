// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_cross_source_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = staged_report_id(&report)?;
    let observation = observation_text(
        &report_id,
        &entry.artifact_id,
        &member_name(&entry.matrix_key, &report_id),
        JOB_ID,
        1,
        OTHER_SHA,
    )?;
    check_invalid_rejected(
        &plan,
        &matrix,
        &report,
        &entry,
        &report_id,
        &observation,
        "cross-source observation was accepted",
    )
}

fn check_tampered_digest_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = staged_report_id(&report)?;
    let member = member_name(&entry.matrix_key, &report_id);
    let content = json!({
        "schema": 1,
        "task_report_id": report_id,
        "artifact_id": entry.artifact_id,
        "member_name": member,
        "producer_job": JOB_ID,
        "producer_attempt": 1,
        "source_sha": HEAD_SHA,
        "observation_session_id": SESSION_ID,
        "unit_ledger_digest": digest_b3(b"gc1-t36-ledger"),
    });
    let content_bytes = canonical_json_bytes(&content)?;
    let binding = json!({
        "schema": 1,
        "task_report_id": report_id,
        "artifact_id": entry.artifact_id,
        "member_name": member,
        "member_size_bytes": content_bytes.len() as u64,
        "member_digest": digest_b3(b"gc1-t36-tampered"),
        "producer_job": JOB_ID,
        "producer_attempt": 1,
        "source_sha": HEAD_SHA,
        "observation_session_id": SESSION_ID,
        "unit_ledger_digest": digest_b3(b"gc1-t36-ledger"),
    });
    check_invalid_rejected(
        &plan,
        &matrix,
        &report,
        &entry,
        &report_id,
        &serde_json::to_string(&binding)?,
        "tampered member digest was accepted",
    )
}

fn check_foreign_artifact_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = staged_report_id(&report)?;
    let foreign = artifact_id_for_final(RUN_KEY)?;
    let observation = observation_text(
        &report_id,
        &foreign,
        &member_name(&entry.matrix_key, &report_id),
        JOB_ID,
        1,
        HEAD_SHA,
    )?;
    check_invalid_rejected(
        &plan,
        &matrix,
        &report,
        &entry,
        &report_id,
        &observation,
        "foreign artifact binding was accepted",
    )
}

fn check_unparsable_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = staged_report_id(&report)?;
    check_invalid_rejected(
        &plan,
        &matrix,
        &report,
        &entry,
        &report_id,
        "{not an observation}",
        "unparsable observation was accepted",
    )
}

#[test]
fn gc1_t36_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_cross_source_rejected()?;
    check_tampered_digest_rejected()?;
    check_foreign_artifact_rejected()?;
    check_unparsable_rejected()?;
    Ok(())
}
