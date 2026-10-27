// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "task_report_public/fixture.rs"]
mod fixture;
#[path = "task_report_public/process.rs"]
mod process;
#[path = "task_report_public/report_files.rs"]
mod report_files;

use velnor_actions_contract::{EdgeKind, FinalStatus, NotSelectedReason, TaskStatus};

use sha2::{Digest, Sha256};
use std::fs;
use std::io;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const MISSING_REPORTS_TEST: &str = "reports_move_not_run_to_passed_through_public_writer";
const BUILD_FAILURE_TEST: &str = "build_failure_generates_blocked_downstream_reports";
const NOOP_TEST: &str = "valid_selected_noop_reports_block_tasks";
const SHARED_ARTIFACT_TEST: &str = "shared_job_artifact_is_downloaded_once";
const PROVIDER_HIT_TEST: &str = "provider_hit_does_not_reclassify_executed_task";
const GENERATOR_WIRE_TEST: &str = "schema_2_generator_sha256_wire_field_is_strict";
#[test]
fn schema_2_generator_sha256_wire_field_is_strict() -> TestResult<()> {
    if process::child_mode(GENERATOR_WIRE_TEST)? {
        return Ok(());
    }
    let jobs = [("crate_clippy", "success")];
    let write = |scenario: &fixture::Scenario, label: &str| {
        report_files::write_task(
            scenario,
            GENERATOR_WIRE_TEST,
            label,
            &scenario.plan.task_ids[0],
            0,
            None,
        )
    };
    let positive = fixture::Scenario::new(&[("clippy", "crate_clippy")])?;
    positive.stage_generator_wire_field("sha256")?;
    let positive_written = write(&positive, "sha256-wire");
    let positive_status = if positive_written.as_ref().is_ok_and(|count| *count == 1) {
        report_files::stage_all(&positive)?;
        let request =
            report_files::assemble(&positive, GENERATOR_WIRE_TEST, "sha256-merge", &jobs)?;
        Some(report_files::merge(&request)?.status)
    } else {
        None
    };
    let negative = fixture::Scenario::new(&[("clippy", "crate_clippy")])?;
    negative.stage_generator_wire_field("digest_b3")?;
    let negative_rejected = write(&negative, "digest-b3-wire").is_err();
    assert!(
        positive_status == Some(FinalStatus::Passed) && negative_rejected,
        "schema-2 sha256 generator must write and merge; digest_b3-only generator must reject"
    );
    Ok(())
}

#[test]
fn reports_move_not_run_to_passed_through_public_writer() -> TestResult<()> {
    if process::child_mode(MISSING_REPORTS_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_test")])?;
    let jobs = [("crate_clippy", "success"), ("crate_test", "success")];
    let missing = report_files::assemble(&scenario, MISSING_REPORTS_TEST, "missing", &jobs)?;
    let before = report_files::merge(&missing)?;
    assert_eq!(before.status, FinalStatus::PlanningFailed);
    assert_eq!(before.counts.executed, 0);
    assert!(before.counts.not_run > 0);

    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        assert_eq!(
            report_files::write_task(
                &scenario,
                MISSING_REPORTS_TEST,
                &format!("writer-{index}"),
                task_id,
                0,
                None,
            )?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let complete = report_files::assemble(&scenario, MISSING_REPORTS_TEST, "complete", &jobs)?;
    let tasks = report_files::task_reports(&complete)?;
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| task.status == TaskStatus::Executed));
    let after = report_files::merge(&complete)?;
    let mut expected_artifacts: Vec<String> = scenario
        .plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.artifact_id.clone())
        .collect();
    expected_artifacts.sort();
    assert_eq!(after.status, FinalStatus::Passed);
    assert_eq!(after.counts.executed, 2);
    assert_eq!(after.counts.not_run, 0);
    assert_eq!(expected_artifacts.len(), 2);
    assert_ne!(expected_artifacts[0], expected_artifacts[1]);
    assert_eq!(after.downloaded_artifact_ids, expected_artifacts);
    Ok(())
}

#[test]
fn build_failure_generates_blocked_downstream_reports() -> TestResult<()> {
    if process::child_mode(BUILD_FAILURE_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[
        ("build", "crate_rust"),
        ("clippy", "crate_rust"),
        ("test", "crate_rust"),
    ])?;
    let build_id = fixture::task_id("build")?;
    assert_eq!(
        report_files::write_task(&scenario, BUILD_FAILURE_TEST, "build", &build_id, 1, None)?,
        3
    );
    report_files::stage_all(&scenario)?;
    let jobs = [("crate_rust", "failure")];
    let request = report_files::assemble(&scenario, BUILD_FAILURE_TEST, "failed-build", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 3);
    let build = tasks
        .iter()
        .find(|task| task.task_id == build_id)
        .ok_or_else(|| io::Error::other("build report missing"))?;
    assert_eq!(build.status, TaskStatus::Failed);
    for kind in ["clippy", "test"] {
        let task_id = fixture::task_id(kind)?;
        let report = tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| io::Error::other("downstream report missing"))?;
        assert_eq!(report.status, TaskStatus::NotSelected);
        assert_eq!(
            report.not_selected_reason,
            Some(NotSelectedReason::UpstreamFailed)
        );
    }
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Failed);
    assert_eq!(final_report.counts.failed, 1);
    assert_eq!(final_report.counts.blocked, 2);
    assert_eq!(final_report.counts.not_run, 0);
    let artifact_id = &scenario.plan.matrix.include[0].artifact_id;
    assert!(
        scenario
            .plan
            .matrix
            .include
            .iter()
            .all(|entry| entry.artifact_id == *artifact_id)
    );
    assert_eq!(
        final_report.downloaded_artifact_ids.as_slice(),
        std::slice::from_ref(artifact_id)
    );
    Ok(())
}

#[test]
fn valid_selected_noop_reports_block_tasks() -> TestResult<()> {
    if process::child_mode(NOOP_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_test")])?;
    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        let digest = scenario.task_digest(task_id)?;
        assert_eq!(
            report_files::write_task(
                &scenario,
                NOOP_TEST,
                &format!("noop-{index}"),
                task_id,
                0,
                Some(digest),
            )?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let jobs = [("crate_clippy", "success"), ("crate_test", "success")];
    let request = report_files::assemble(&scenario, NOOP_TEST, "noop", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .all(|task| task.status == TaskStatus::NotSelected)
    );
    assert!(
        tasks
            .iter()
            .all(|task| { task.not_selected_reason == Some(NotSelectedReason::NotInPlan) })
    );
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Blocked);
    assert_eq!(final_report.counts.blocked, 2);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.not_run, 0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn shared_job_artifact_is_downloaded_once() -> TestResult<()> {
    if process::child_mode(SHARED_ARTIFACT_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_clippy")])?;
    let entries = &scenario.plan.matrix.include;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].artifact_id, entries[1].artifact_id);
    assert_ne!(entries[0].matrix_key, entries[1].matrix_key);
    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        assert_eq!(
            report_files::write_task(
                &scenario,
                SHARED_ARTIFACT_TEST,
                &format!("shared-{index}"),
                task_id,
                0,
                None,
            )?,
            1
        );
    }
    let payload = scenario.root().join("artifact-payload");
    fs::create_dir_all(&payload)?;
    report_files::stage_artifact_payload(&scenario, &payload)?;
    let mise_data_dir = fixture::install_fake_gh(&scenario)?;
    let fake_mise_bin = report_files::install_fake_mise(&scenario)?;
    let request_log = scenario.root().join("gh-requests.tsv");
    assert_eq!(
        report_files::retrieve(
            &scenario,
            SHARED_ARTIFACT_TEST,
            "retrieve-shared",
            &mise_data_dir,
            &fake_mise_bin,
            &payload,
            &request_log,
        )?,
        1
    );
    let expected_request = format!(
        "{}\t{}\ttailrocks/velnor-new\n",
        fixture::RUN_ID,
        entries[0].artifact_id
    );
    assert_eq!(fs::read_to_string(request_log)?, expected_request);
    let jobs = [("crate_clippy", "success")];
    let request = report_files::assemble(&scenario, SHARED_ARTIFACT_TEST, "shared", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 2);
    for task_id in &scenario.plan.task_ids {
        let task = tasks
            .iter()
            .find(|task| task.task_id == *task_id)
            .ok_or_else(|| io::Error::other("retrieved task report missing"))?;
        assert_eq!(task.status, TaskStatus::Executed);
    }
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    assert_eq!(final_report.counts.executed, 2);
    assert_eq!(
        final_report.downloaded_artifact_ids,
        [entries[0].artifact_id.clone()]
    );
    Ok(())
}

#[test]
fn provider_hit_does_not_reclassify_executed_task() -> TestResult<()> {
    if process::child_mode(PROVIDER_HIT_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::tofu_provider()?;
    let init_task_id = velnor_actions_tofu::task_id_for_root(
        "infra",
        velnor_actions_tofu::TofuTaskKind::InitForValidate,
        "default",
    )?;
    let validate_task_id = velnor_actions_tofu::task_id_for_root(
        "infra",
        velnor_actions_tofu::TofuTaskKind::Validate,
        "default",
    )?;
    assert!(scenario.plan.task_ids.contains(&init_task_id));
    assert!(scenario.plan.task_ids.contains(&validate_task_id));
    assert_eq!(scenario.plan.edges.len(), 1);
    assert_eq!(scenario.plan.edges[0].from, init_task_id);
    assert_eq!(scenario.plan.edges[0].to, validate_task_id);
    assert_eq!(scenario.plan.edges[0].kind, EdgeKind::Data);
    for (label, task_id) in [
        ("tofu-init", init_task_id.as_str()),
        ("tofu-validate", validate_task_id.as_str()),
    ] {
        assert_eq!(
            report_files::write_task(&scenario, PROVIDER_HIT_TEST, label, task_id, 0, None)?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let lockfile = fs::read(scenario.root().join("infra/.terraform.lock.hcl"))?;
    let cache_key = tofu_provider_key(&lockfile)?;
    report_files::stage_provider_hit(&scenario, &validate_task_id, &cache_key)?;
    let jobs = [(scenario.plan.jobs[0].job_id.as_str(), "success")];
    let request = report_files::assemble(&scenario, PROVIDER_HIT_TEST, "provider-merge", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 2);
    let init = tasks
        .iter()
        .find(|task| task.task_id == init_task_id)
        .ok_or_else(|| io::Error::other("init report missing"))?;
    let validate = tasks
        .iter()
        .find(|task| task.task_id == validate_task_id)
        .ok_or_else(|| io::Error::other("validate report missing"))?;
    assert_eq!(init.status, TaskStatus::Executed);
    assert_eq!(validate.status, TaskStatus::Executed);
    assert_eq!(
        validate.cache.layer,
        velnor_actions_contract::CacheLayer::TofuProviders
    );
    assert_eq!(
        validate.cache.result,
        velnor_actions_contract::CacheResult::Hit
    );
    assert_eq!(validate.cache.key, cache_key);
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    assert_eq!(final_report.counts.selected, 2);
    assert_eq!(final_report.counts.executed, 2);
    assert_eq!(final_report.counts.reused, 0);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.cancelled, 0);
    assert_eq!(final_report.counts.blocked, 0);
    assert_eq!(final_report.counts.not_run, 0);
    assert_eq!(
        final_report.downloaded_artifact_ids,
        [scenario.plan.matrix.include[0].artifact_id.clone()]
    );
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

fn tofu_provider_key(lockfile: &[u8]) -> TestResult<String> {
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
