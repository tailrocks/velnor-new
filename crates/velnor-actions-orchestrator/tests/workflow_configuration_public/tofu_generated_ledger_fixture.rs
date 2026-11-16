// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::workflow::group_ledger::{EmitEnv, JobStatus, emit_group_ledger_to};
use velnor_actions_contract::workflow::{
    PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason, UnitLedgerV1,
};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskRef, ObligationDecision, Plan,
    TASK_EXECUTION_MANIFEST_PATH, TaskReport, TaskStatus, canonical_json_bytes, plan_json_bytes,
    task_report_id_for_task,
};
use velnor_actions_orchestrator::{plan_internal, prepare, render_staged_tree};

use super::tofu_providers_producers::tofu_repo;
use super::workflow_configuration_fixture::git;

pub(super) type TestResult<T = ()> = Result<T, Box<dyn Error>>;
pub(super) const RUN_KEY: &str = "r190033-a1";

pub(super) struct Fixture {
    pub(super) repo: TempDir,
    pub(super) plan: Plan,
}

impl Fixture {
    pub(super) fn new(mixed: bool) -> TestResult<Self> {
        let repo = tofu_repo(&[if mixed { "infra" } else { "." }])?;
        let root = repo.path();
        if !mixed {
            fs::remove_file(root.join("Cargo.toml"))?;
            fs::remove_dir_all(root.join("src"))?;
        }
        let prepared = prepare(root)?;
        let tree = render_staged_tree(&prepared)?;
        for file in tree.files {
            let path = root.join(file.path);
            fs::create_dir_all(path.parent().ok_or("generated file parent missing")?)?;
            fs::write(path, file.bytes)?;
        }
        git(&["add", "-A"], root)?;
        git(&["commit", "-qm", "fixture"], root)?;
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()?;
        if !output.status.success() {
            return Err("fixture HEAD missing".into());
        }
        let head = String::from_utf8(output.stdout)?.trim().to_owned();
        let response: serde_json::Value = serde_json::from_str(&plan_internal(
            &json!({"schema":1,"op":"plan-v1","run_key":RUN_KEY,"head":head,"event":"push","root":root})
                .to_string(),
        )?)?;
        let plan: Plan = serde_json::from_value(response["plan"].clone())?;
        plan.validate()?;
        Ok(Self { repo, plan })
    }

    pub(super) fn manifest_path(&self) -> PathBuf {
        self.repo.path().join(TASK_EXECUTION_MANIFEST_PATH)
    }

    pub(super) fn expected(&self, job: &str) -> BTreeSet<String> {
        self.plan
            .obligations
            .iter()
            .filter(|item| item.job_id == job && item.decision == ObligationDecision::Execute)
            .map(|item| item.task_id.clone())
            .collect()
    }

    pub(super) fn emit(&self, job: &str) -> TestResult<UnitLedgerV1> {
        let runner = tempfile::tempdir()?;
        let run_dir = runner.path().join("velnor").join(RUN_KEY);
        fs::create_dir_all(&run_dir)?;
        fs::write(run_dir.join("plan.json"), plan_json_bytes(&self.plan)?)?;
        self.stage_reports(&run_dir)?;
        let env = EmitEnv {
            job_key: job.to_owned(),
            run_id: 190_033,
            run_attempt: 1,
            event: "push".to_owned(),
            source_sha: self.plan.head.clone(),
            ref_name: "refs/heads/testmain".to_owned(),
            repository: "example/tofu".to_owned(),
            workflow_sha: self.plan.head.clone(),
            matrix_key: None,
            job_status: JobStatus::Success,
        };
        let path = emit_group_ledger_to(
            runner.path(),
            &self.manifest_path(),
            &run_dir,
            RUN_KEY,
            &env,
        )?;
        let ledger: UnitLedgerV1 = serde_json::from_slice(&fs::read(path)?)?;
        ledger.validate()?;
        Ok(ledger)
    }

    fn stage_reports(&self, run_dir: &std::path::Path) -> TestResult {
        for entry in &self.plan.matrix.include {
            let directory = run_dir.join(&entry.matrix_key).join("tasks");
            fs::create_dir_all(&directory)?;
            let ids: BTreeSet<&str> = entry
                .execute_task_ids
                .tasks
                .values()
                .flat_map(|task| match task {
                    ExecuteTaskRef::Single(id) => vec![id.as_str()],
                    ExecuteTaskRef::Shards(ids) => ids.iter().map(String::as_str).collect(),
                })
                .collect();
            for id in ids {
                let obligation = self
                    .plan
                    .obligations
                    .iter()
                    .find(|item| item.task_id == id)
                    .ok_or("plan obligation missing")?;
                let report_id =
                    task_report_id_for_task(RUN_KEY, &entry.matrix_key, &obligation.task_digest)?;
                let report = TaskReport {
                    schema: TaskReport::SCHEMA,
                    task_report_id: report_id.clone(),
                    run_key: RUN_KEY.to_owned(),
                    event: self.plan.event,
                    trust: self.plan.trust,
                    matrix_id: entry.id.clone(),
                    matrix_key: entry.matrix_key.clone(),
                    task_id: id.to_owned(),
                    task_digest: obligation.task_digest.clone(),
                    status: TaskStatus::Executed,
                    not_selected_reason: None,
                    cache: CacheOutcome {
                        layer: CacheLayer::Task,
                        key: String::new(),
                        result: CacheResult::NotAttempted,
                        miss_reason: None,
                    },
                    platform_binding: PlatformBinding::unavailable(
                        &entry.planned_platform.platform_id,
                        PlatformRunnerEnvironment::GithubHosted,
                        PlatformUnavailableReason::MissingImageMetadata,
                    )?,
                    exit_code: 0,
                    duration_ms: Some(1),
                    outputs: Vec::new(),
                    lane: None,
                    queue: None,
                    partition: None,
                    reason: None,
                    timing: None,
                };
                report.validate()?;
                fs::write(
                    directory.join(format!("{report_id}.json")),
                    canonical_json_bytes(&report)?,
                )?;
            }
        }
        Ok(())
    }
}
