// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io;

use sha2::{Digest as _, Sha256};
use velnor_actions_contract::workflow::{
    ExecutionBranch, JobBindingV1, UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::{
    Digest, ObligationDecision, TaskReport, canonical_json_bytes, digest_b3, plan_json_bytes,
};

use super::TestResult;
use super::fixture::{RUN_ATTEMPT, RUN_ID, Scenario};

pub(super) fn stage_crate_job_ledger(
    scenario: &Scenario,
    reports: &[TaskReport],
) -> TestResult<()> {
    let plan = &scenario.plan;
    let job_key = plan
        .jobs
        .first()
        .map(|job| job.job_id.clone())
        .ok_or_else(|| io::Error::other("tofu crate job missing"))?;
    let plan_digest = UnitLedgerV1::plan_digest_from_canonical_bytes(&plan_json_bytes(plan)?);
    let mut units: Vec<String> = plan
        .obligations
        .iter()
        .filter(|obligation| {
            obligation.job_id == job_key && obligation.decision == ObligationDecision::Execute
        })
        .map(|obligation| obligation.task_id.clone())
        .collect();
    units.sort();
    let digest =
        |unit: &str, part: &str| Digest::parse(&digest_b3(format!("{unit}/{part}").as_bytes()));
    let mut ordered_results = Vec::with_capacity(units.len());
    for unit in &units {
        let report = reports
            .iter()
            .find(|report| report.task_id == *unit)
            .ok_or_else(|| io::Error::other("tofu task report missing"))?;
        ordered_results.push(UnitResultV1 {
            unit_id: unit.clone(),
            recipe_id: digest(unit, "recipe")?,
            task_identity: unit.clone(),
            argv_digest: digest(unit, "argv")?,
            effective_input_digest: digest(unit, "input")?,
            outcome: UnitOutcome::Executed,
            exit_code: Some(0),
            started_at: "2026-11-02T00:00:00Z".to_owned(),
            finished_at: "2026-11-02T00:00:01Z".to_owned(),
            report_digest: UnitResultV1::report_digest_from_canonical_bytes(&canonical_json_bytes(
                report,
            )?),
        });
    }
    let absent = vec![None; units.len()];
    let ledger = UnitLedgerV1 {
        schema: 1,
        job: JobBindingV1 {
            schema: 1,
            repository_id: 0,
            repository: "tailrocks/velnor-new".to_owned(),
            run_id: RUN_ID.parse()?,
            run_attempt: RUN_ATTEMPT.parse()?,
            job_id: 0,
            job_key: job_key.clone(),
            event: "pull_request".to_owned(),
            ref_name: "refs/pull/1/merge".to_owned(),
            workflow_sha: plan.head.clone(),
            source_sha: plan.head.clone(),
            plan_digest: plan_digest.clone(),
        },
        group_digest: UnitLedgerV1::group_digest_for_group(&job_key, &plan_digest)?,
        execution_branch: ExecutionBranch::Reader,
        selected_unit_ids: units,
        ordered_results,
        observation_digest: UnitLedgerV1::observation_digest_for_sidecars(&absent)?,
        source_binding_digest: UnitLedgerV1::source_binding_digest_for_sidecars(&absent)?,
    };
    ledger.validate()?;
    let ledgers = serde_json::Value::Array(vec![serde_json::to_value(&ledger)?]);
    fs::write(
        scenario.run_dir.join("group-ledgers.json"),
        serde_json::to_vec(&ledgers)?,
    )?;
    Ok(())
}

fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let value = *byte;
        encoded.push(char::from(HEX[usize::from(value >> 4)]));
        encoded.push(char::from(HEX[usize::from(value & 0x0f)]));
    }
    encoded
}

pub(super) fn tofu_provider_key(lockfile: &[u8]) -> TestResult<String> {
    let family = velnor_actions_contract::workflow::cache_family::CacheFamily::TofuProviders;
    let locator = velnor_actions_tofu::tofu_root_locator("infra")?;
    let compatibility =
        Sha256::digest(format!("{locator}\n{}", velnor_actions_mise::OPENTOFU_VERSION).as_bytes());
    let snapshot = Sha256::digest(lockfile);
    let key = format!(
        "{}trusted-s256-{}-s256-{}",
        family.key_prefix(),
        digest_hex(compatibility),
        digest_hex(snapshot)
    );
    let payload = key
        .strip_prefix(&format!("{}trusted-s256-", family.key_prefix()))
        .ok_or_else(|| io::Error::other("provider key prefix mismatch"))?;
    let (compatibility, snapshot) = payload
        .split_once("-s256-")
        .ok_or_else(|| io::Error::other("provider key snapshot missing"))?;
    for digest in [compatibility, snapshot] {
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }
    assert_eq!(
        velnor_actions_contract::workflow::cache_family::CacheFamily::for_key(&key),
        Some(family)
    );
    assert!(key.len() <= family.max_key_bytes());
    assert!(key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES);
    Ok(key)
}
