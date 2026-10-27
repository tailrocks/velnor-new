// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod assemble;
mod emit;
mod emit_read;

pub use assemble::{ASSEMBLE_GROUP_LEDGERS_OP, assemble_group_ledgers};
pub use emit::{EMIT_GROUP_LEDGER_OP, emit_group_ledger};

// gc1-t54 fixture support: synthetic run dirs (plan.json, staged
// TaskReports, task execution manifest) for the emit/assemble tests.
#[cfg(test)]
pub(crate) mod gc1_t54_support {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
    use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
    use velnor_actions_contract::workflow::{
        TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
    };
    use velnor_actions_contract::{
        CacheLayer, CacheOutcome, CacheResult, ExecuteTaskIds, ExecuteTaskRef, JobDisposition,
        MatrixEntry, ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation,
        PlanRunner, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment,
        PlatformUnavailableReason, RunnerSelection, TaskReport, TaskStatus, Trust, WorkflowEvent,
        digest_b3, matrix_key_for_id, plan_id_for_run, plan_json_bytes, task_id_for_internal,
        task_report_id_for_task, trust_for_event,
    };

    use super::emit::{EmitEnv, JobStatus};
    use crate::OrchestratorError;

    pub(crate) const RUN_KEY: &str = "r4242-a1";
    pub(crate) const GROUP: &str = "t54-group";
    pub(crate) const HEAD_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    pub(crate) const WORKFLOW_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    pub(crate) struct Fixture {
        pub(crate) _tmp: tempfile::TempDir,
        pub(crate) anchor: PathBuf,
        pub(crate) manifest_path: PathBuf,
        pub(crate) run_dir: PathBuf,
        pub(crate) emit_env: EmitEnv,
        pub(crate) task_id: String,
        pub(crate) report_id: String,
        pub(crate) matrix_key: String,
    }

    struct Parts {
        task_id: String,
        task_digest: String,
        input_digest: String,
        closure_digest: String,
        planned: PlannedPlatform,
        entry: MatrixEntry,
        matrix_key: String,
    }

    fn fail(problem: &str) -> OrchestratorError {
        crate::internal::internal(problem)
    }

    pub(crate) fn build(status: JobStatus) -> Result<Fixture, OrchestratorError> {
        let tmp = tempfile::tempdir().map_err(|err| fail(&err.to_string()))?;
        let anchor = tmp.path().to_path_buf();
        let run_dir = anchor.join("velnor").join(RUN_KEY);
        let parts = fixture_parts()?;
        let plan = fixture_plan(&parts)?;
        let report_id = task_report_id_for_task(RUN_KEY, &parts.matrix_key, &parts.task_digest)
            .map_err(|_| fail("report_id"))?;
        let report = fixture_report(&parts, &report_id)?;
        let manifest = fixture_manifest(&parts)?;
        write_fixture(
            &anchor, &run_dir, &plan, &parts, &report_id, &report, &manifest,
        )?;
        let manifest_path = anchor.join("task-execution-manifest-v1.json");
        Ok(Fixture {
            _tmp: tmp,
            anchor,
            manifest_path,
            run_dir,
            emit_env: EmitEnv {
                job_key: format!("{GROUP}-write"),
                run_id: 4242,
                run_attempt: 1,
                event: "pull_request".to_owned(),
                source_sha: HEAD_SHA.to_owned(),
                ref_name: "refs/pull/1/merge".to_owned(),
                repository: "tailrocks/velnor-new".to_owned(),
                workflow_sha: WORKFLOW_SHA.to_owned(),
                matrix_key: Some(parts.matrix_key.clone()),
                job_status: status,
            },
            task_id: parts.task_id.clone(),
            report_id,
            matrix_key: parts.matrix_key.clone(),
        })
    }

    fn fixture_parts() -> Result<Parts, OrchestratorError> {
        let task_id = task_id_for_internal("lint", "dev").map_err(|_| fail("task_id"))?;
        let task_digest = digest_b3(b"gc1-t54-task-payload");
        let input_digest = digest_b3(b"gc1-t54-input");
        let closure_digest = digest_b3(b"gc1-t54-closure");
        let planned = PlannedPlatform::new("ubuntu-22.04", "x86_64-unknown-linux-gnu")
            .map_err(|_| fail("planned_platform"))?;
        let execute = ExecuteTaskIds {
            tasks: BTreeMap::from([("group".to_owned(), ExecuteTaskRef::Single(task_id.clone()))]),
        };
        let entry = MatrixEntry::derive(
            "rust",
            &task_id,
            "ci",
            &task_digest,
            serde_json::json!({}),
            execute,
            &input_digest,
            RUN_KEY,
            GROUP,
            planned.clone(),
        )
        .map_err(|_| fail("matrix_entry"))?;
        Ok(Parts {
            task_id,
            task_digest,
            input_digest,
            closure_digest,
            planned,
            matrix_key: entry.matrix_key.clone(),
            entry,
        })
    }

    fn fixture_plan(parts: &Parts) -> Result<Plan, OrchestratorError> {
        let plan = Plan {
            schema: Plan::SCHEMA,
            run_key: RUN_KEY.to_owned(),
            plan_id: plan_id_for_run(RUN_KEY).map_err(|_| fail("plan_id"))?,
            base: None,
            head: HEAD_SHA.to_owned(),
            event: WorkflowEvent::PullRequest,
            qualification: None,
            runner: PlanRunner {
                label: "ubuntu-22.04".to_owned(),
                selection: RunnerSelection::LatestDefault,
            },
            trust: trust_for_event(WorkflowEvent::PullRequest),
            generator: PlanGenerator {
                version: "0.1.55".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
                sha256: "6dd871a3ac5736745b77359310384c1932ce0a7f921e06207a99d6710dcd47".to_owned(),
            },
            packages: Vec::new(),
            obligations: vec![PlanObligation {
                task_id: parts.task_id.clone(),
                decision: ObligationDecision::Execute,
                reason: "gc1-t54".to_owned(),
                task_digest: parts.task_digest.clone(),
                input_digest: parts.input_digest.clone(),
                closure_digest: parts.closure_digest.clone(),
                job_id: GROUP.to_owned(),
            }],
            matrix: PlanMatrix {
                include: vec![parts.entry.clone()],
            },
            task_ids: vec![parts.task_id.clone()],
            jobs: vec![PlanJob {
                job_id: GROUP.to_owned(),
                disposition: JobDisposition::Run,
            }],
            warnings: Vec::new(),
            edges: Vec::new(),
        };
        plan.validate().map_err(|_| fail("plan_invalid"))?;
        Ok(plan)
    }

    fn fixture_report(parts: &Parts, report_id: &str) -> Result<TaskReport, OrchestratorError> {
        let report = TaskReport {
            schema: TaskReport::SCHEMA,
            task_report_id: report_id.to_owned(),
            run_key: RUN_KEY.to_owned(),
            event: WorkflowEvent::PullRequest,
            trust: Trust::Pr,
            matrix_id: parts.entry.id.clone(),
            matrix_key: parts.matrix_key.clone(),
            task_id: parts.task_id.clone(),
            task_digest: parts.task_digest.clone(),
            status: TaskStatus::Executed,
            not_selected_reason: None,
            cache: CacheOutcome {
                layer: CacheLayer::Task,
                key: String::new(),
                result: CacheResult::NotAttempted,
                miss_reason: None,
            },
            platform_binding: PlatformBinding::unavailable(
                &parts.planned.platform_id,
                PlatformRunnerEnvironment::GithubHosted,
                PlatformUnavailableReason::MissingImageMetadata,
            )
            .map_err(|_| fail("platform_binding"))?,
            exit_code: 0,
            duration_ms: Some(1_500),
            outputs: Vec::new(),
            lane: None,
            queue: None,
            partition: None,
            reason: None,
            timing: None,
        };
        report.validate().map_err(|_| fail("report_invalid"))?;
        Ok(report)
    }

    fn fixture_manifest(parts: &Parts) -> Result<TaskExecutionManifestV1, OrchestratorError> {
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
        let task_digest = task_digest_for_execution(
            &parts.task_id,
            &argv,
            &toolchain_id(&toolchain_inputs).map_err(|_| fail("toolchain_id"))?,
        )
        .map_err(|_| fail("manifest_task_digest"))?;
        let matrix_id = format!("stack:rust|task:{}", parts.task_id);
        let matrix_key = matrix_key_for_id(&matrix_id).map_err(|_| fail("manifest_matrix_key"))?;
        let mut entry = TaskExecutionManifestEntryV1 {
            task_id: parts.task_id.clone(),
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
            report_helper_version: "0.1.57".to_owned(),
            matrix_max_parallel: None,
        };
        entry
            .refresh_execution_digest()
            .map_err(|_| fail("execution_digest"))?;
        let manifest = TaskExecutionManifestV1 {
            schema: TASK_EXECUTION_MANIFEST_SCHEMA,
            generator_version: "0.1.57".to_owned(),
            tasks: BTreeMap::from([(parts.task_id.clone(), entry)]),
        };
        manifest.validate().map_err(|_| fail("manifest_invalid"))?;
        Ok(manifest)
    }

    fn write_fixture(
        anchor: &std::path::Path,
        run_dir: &std::path::Path,
        plan: &Plan,
        parts: &Parts,
        report_id: &str,
        report: &TaskReport,
        manifest: &TaskExecutionManifestV1,
    ) -> Result<(), OrchestratorError> {
        let tasks_dir = run_dir.join(&parts.matrix_key).join("tasks");
        std::fs::create_dir_all(&tasks_dir).map_err(|err| fail(&err.to_string()))?;
        let plan_bytes = plan_json_bytes(plan).map_err(|_| fail("plan_bytes"))?;
        std::fs::write(run_dir.join("plan.json"), &plan_bytes)
            .map_err(|err| fail(&err.to_string()))?;
        let report_bytes = velnor_actions_contract::canonical_json_bytes(report)
            .map_err(|_| fail("report_bytes"))?;
        std::fs::write(tasks_dir.join(format!("{report_id}.json")), &report_bytes)
            .map_err(|err| fail(&err.to_string()))?;
        let marked = manifest
            .marked_json()
            .map_err(|_| fail("manifest_marked"))?;
        std::fs::write(anchor.join("task-execution-manifest-v1.json"), marked)
            .map_err(|err| fail(&err.to_string()))?;
        Ok(())
    }
}
