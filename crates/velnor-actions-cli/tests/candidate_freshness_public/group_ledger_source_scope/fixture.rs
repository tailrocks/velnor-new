// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::workflow::{
    TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
    UnitLedgerV1,
};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ExecuteTaskIds, ExecuteTaskRef, JobDisposition,
    MatrixEntry, ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation,
    PlanRunner, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
    PlatformUnavailableReason, QualificationDispatch, QualificationPhase, RunnerSelection,
    TaskReport, TaskStatus, canonical_json_bytes, digest_b3, matrix_key_for_id, plan_id_for_run,
    plan_json_bytes, run_key_for_ci, task_id_for_internal, task_report_id_for_task,
    trust_for_event,
};

use super::{BASE_SHA, GROUP, Kind, MERGE_SHA, REPOSITORY, RUN_ATTEMPT, RUN_ID, WORKFLOW_SHA};

pub(super) struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    runner_temp: PathBuf,
    pub(super) event_path: PathBuf,
    request_path: PathBuf,
    matrix_key: String,
}

impl Fixture {
    pub(super) fn new(kind: Kind) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let repo = temp.path().join("repo");
        let runner_temp = temp.path().join("runner-temp");
        let event_path = temp.path().join("event.json");
        let request_path = runner_temp.join("velnor/request/plan-v1-request.json");
        fs::create_dir_all(repo.join(".github/velnor"))?;
        fs::create_dir_all(&runner_temp)?;
        fs::write(&event_path, kind.payload().to_string())?;
        let run_key = run_key_for_ci(RUN_ID, RUN_ATTEMPT);
        let parts = Parts::new(&run_key)?;
        let plan = parts.plan(kind)?;
        let report = parts.report(kind)?;
        let manifest = parts.manifest()?;
        let run_dir = runner_temp.join("velnor").join(&run_key);
        let tasks_dir = run_dir.join(&parts.matrix_key).join("tasks");
        fs::create_dir_all(&tasks_dir)?;
        fs::write(run_dir.join("plan.json"), plan_json_bytes(&plan)?)?;
        fs::write(
            tasks_dir.join(format!("{}.json", parts.report_id)),
            canonical_json_bytes(&report)?,
        )?;
        fs::write(
            repo.join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH),
            manifest.marked_json()?,
        )?;
        Ok(Self {
            temp,
            repo,
            runner_temp,
            event_path,
            request_path,
            matrix_key: parts.matrix_key,
        })
    }

    pub(super) fn write_request(&self, kind: Kind) -> Result<serde_json::Value, Box<dyn Error>> {
        let output = self.invoke("write-request-v1", kind.request_event(), MERGE_SHA)?;
        success(&output, "public native request writer")?;
        Ok(serde_json::from_slice(&fs::read(&self.request_path)?)?)
    }

    pub(super) fn emit(&self, event_name: &str) -> Result<Output, Box<dyn Error>> {
        self.invoke("emit-group-ledger-v1", event_name, MERGE_SHA)
    }

    fn invoke(
        &self,
        operation: &str,
        event_name: &str,
        github_sha: &str,
    ) -> Result<Output, Box<dyn Error>> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        let github_ref = if matches!(event_name, "workflow_dispatch" | "push") {
            "refs/heads/main"
        } else {
            "refs/pull/17/merge"
        };
        let workflow_sha = if event_name == "workflow_dispatch" {
            MERGE_SHA
        } else {
            WORKFLOW_SHA
        };
        command
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.temp.path().join("home"))
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("GITHUB_RUN_ID", RUN_ID.to_string())
            .env("GITHUB_RUN_ATTEMPT", RUN_ATTEMPT.to_string())
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_JOB", format!("{GROUP}-write"))
            .env("GITHUB_WORKFLOW_SHA", workflow_sha)
            .env(
                "GITHUB_WORKFLOW_REF",
                format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
            )
            .env("GITHUB_REF_PROTECTED", "true")
            .env("GITHUB_REF", github_ref)
            .env("GITHUB_EVENT_NAME", event_name)
            .env("GITHUB_SHA", github_sha)
            .env("GITHUB_EVENT_PATH", &self.event_path)
            .env("VELNOR_INTERNAL_OP", operation)
            .env("VELNOR_REQUEST_FILE", &self.request_path)
            .env("VELNOR_MATRIX_KEY", &self.matrix_key)
            .env("VELNOR_JOB_STATUS", "success");
        Ok(command.output()?)
    }

    pub(super) fn ledger_path(&self) -> PathBuf {
        self.runner_temp
            .join("velnor")
            .join(run_key_for_ci(RUN_ID, RUN_ATTEMPT))
            .join(&self.matrix_key)
            .join("group-ledger.json")
    }

    pub(super) fn read_ledger(&self) -> Result<UnitLedgerV1, Box<dyn Error>> {
        Ok(UnitLedgerV1::parse_json(&fs::read_to_string(
            self.ledger_path(),
        )?)?)
    }
}

struct Parts {
    run_key: String,
    task_id: String,
    task_digest: String,
    input_digest: String,
    closure_digest: String,
    planned: PlannedPlatform,
    entry: MatrixEntry,
    matrix_key: String,
    report_id: String,
}

impl Parts {
    fn new(run_key: &str) -> Result<Self, Box<dyn Error>> {
        let task_id = task_id_for_internal("lint", "source-scope")?;
        let task_digest = digest_b3(b"group-ledger-source-scope-task");
        let input_digest = digest_b3(b"group-ledger-source-scope-input");
        let closure_digest = digest_b3(b"group-ledger-source-scope-closure");
        let planned = PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?;
        let execute = ExecuteTaskIds {
            tasks: BTreeMap::from([("group".to_owned(), ExecuteTaskRef::Single(task_id.clone()))]),
        };
        let entry = MatrixEntry::derive(
            "rust",
            &task_id,
            "ci",
            &task_digest,
            json!({}),
            execute,
            &input_digest,
            run_key,
            GROUP,
            planned.clone(),
        )?;
        let matrix_key = entry.matrix_key.clone();
        let report_id = task_report_id_for_task(run_key, &matrix_key, &task_digest)?;
        Ok(Self {
            run_key: run_key.to_owned(),
            task_id,
            task_digest,
            input_digest,
            closure_digest,
            planned,
            entry,
            matrix_key,
            report_id,
        })
    }

    fn plan(&self, kind: Kind) -> Result<Plan, Box<dyn Error>> {
        let event = kind.logical_event();
        let qualification = if matches!(kind, Kind::Qualification) {
            Some(QualificationDispatch {
                campaign: "source-scope".to_owned(),
                phase: QualificationPhase::Cold,
                repository: REPOSITORY.to_owned(),
                default_branch: "main".to_owned(),
                git_ref: "refs/heads/main".to_owned(),
                ref_protected: true,
                workflow_ref: format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
                workflow_sha: kind.logical_head().to_owned(),
                source_sha: kind.logical_head().to_owned(),
                run_id: RUN_ID,
                run_attempt: u32::try_from(RUN_ATTEMPT)?,
                predecessor: None,
            })
        } else {
            None
        };
        let plan = Plan {
            schema: Plan::SCHEMA,
            run_key: self.run_key.clone(),
            plan_id: plan_id_for_run(&self.run_key)?,
            base: matches!(kind, Kind::PullRequest | Kind::Fork).then(|| BASE_SHA.to_owned()),
            head: kind.logical_head().to_owned(),
            event,
            qualification,
            runner: PlanRunner {
                label: "ubuntu-26.04".to_owned(),
                selection: RunnerSelection::LatestDefault,
            },
            trust: trust_for_event(event),
            generator: PlanGenerator {
                version: "0.1.60".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
                sha256: "6dd871a3ac5736745b77359310384c1932ce0a7f921e06207a99d6710dcd47".to_owned(),
            },
            packages: Vec::new(),
            obligations: vec![PlanObligation {
                task_id: self.task_id.clone(),
                decision: ObligationDecision::Execute,
                reason: "source-scope-regression".to_owned(),
                task_digest: self.task_digest.clone(),
                input_digest: self.input_digest.clone(),
                closure_digest: self.closure_digest.clone(),
                job_id: GROUP.to_owned(),
            }],
            matrix: PlanMatrix {
                include: vec![self.entry.clone()],
            },
            task_ids: vec![self.task_id.clone()],
            jobs: vec![PlanJob {
                job_id: GROUP.to_owned(),
                disposition: JobDisposition::Run,
            }],
            warnings: Vec::new(),
            edges: Vec::new(),
        };
        plan.validate()?;
        Ok(plan)
    }

    fn report(&self, kind: Kind) -> Result<TaskReport, Box<dyn Error>> {
        let event = kind.logical_event();
        let report = TaskReport {
            schema: TaskReport::SCHEMA,
            task_report_id: self.report_id.clone(),
            run_key: self.run_key.clone(),
            event,
            trust: trust_for_event(event),
            matrix_id: self.entry.id.clone(),
            matrix_key: self.matrix_key.clone(),
            task_id: self.task_id.clone(),
            task_digest: self.task_digest.clone(),
            status: TaskStatus::Executed,
            not_selected_reason: None,
            cache: CacheOutcome {
                layer: CacheLayer::Task,
                key: String::new(),
                result: CacheResult::NotAttempted,
                miss_reason: None,
            },
            platform_binding: PlatformBinding::unavailable(
                &self.planned.platform_id,
                PlatformRunnerEnvironment::GithubHosted,
                PlatformUnavailableReason::MissingImageMetadata,
            )?,
            exit_code: 0,
            duration_ms: Some(1_500),
            outputs: Vec::new(),
            lane: None,
            queue: None,
            partition: None,
            reason: None,
            timing: None,
        };
        report.validate()?;
        Ok(report)
    }

    fn manifest(&self) -> Result<TaskExecutionManifestV1, Box<dyn Error>> {
        let toolchain_inputs = ToolchainInputs {
            tools: vec!["rust@1.89.0".to_owned()],
            components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_test".to_owned(),
        };
        let argv = vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "exec".to_owned(),
            "rust@1.89.0".to_owned(),
            "--".to_owned(),
            "cargo".to_owned(),
            "test".to_owned(),
            "--locked".to_owned(),
        ];
        let task_digest =
            task_digest_for_execution(&self.task_id, &argv, &toolchain_id(&toolchain_inputs)?)?;
        let matrix_id = format!("stack:rust|task:{}", self.task_id);
        let matrix_key = matrix_key_for_id(&matrix_id)?;
        let mut entry = TaskExecutionManifestEntryV1 {
            task_id: self.task_id.clone(),
            execution_digest: String::new(),
            task_digest,
            toolchain_inputs,
            argv,
            env: BTreeMap::from([
                ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
                ("MISE_NO_ENV".to_owned(), "1".to_owned()),
                ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
                ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
                ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
                ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
                (
                    "MISE_RUSTUP_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/rustup".to_owned(),
                ),
                (
                    "MISE_CARGO_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/cargo".to_owned(),
                ),
                (
                    "RUSTUP_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/rustup".to_owned(),
                ),
                (
                    "CARGO_HOME".to_owned(),
                    "${{ runner.temp }}/velnor/cargo".to_owned(),
                ),
            ]),
            matrix_id,
            matrix_key,
            report_helper_version: "0.1.60".to_owned(),
            matrix_max_parallel: None,
        };
        entry.refresh_execution_digest()?;
        let manifest = TaskExecutionManifestV1 {
            schema: TASK_EXECUTION_MANIFEST_SCHEMA,
            generator_version: "0.1.60".to_owned(),
            tasks: BTreeMap::from([(self.task_id.clone(), entry)]),
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

fn success(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed: {}", String::from_utf8_lossy(&output.stderr)).into())
    }
}
