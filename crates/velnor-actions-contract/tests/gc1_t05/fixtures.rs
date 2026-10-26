// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::{
    matrix_id_for_task_group, matrix_key_for_id, run_key_for_ci, task_id_for_internal,
    task_report_id_for_task,
};
use velnor_actions_contract::workflow::cache_context::{
    ApiConclusion, CacheCapability, CacheDriver, CompilerAdapterV1, EffectiveJobBindingV1,
    ExecutionBranch, ExecutionContextV1, JobBindingV1, LaneContextV1, MeasuredU64,
    ObservationGapCode, ObservationGapV1, SourceBindingV1, UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::workflow::platform::{
    PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason,
};
use velnor_actions_contract::workflow::report::{
    CacheLayer, CacheOutcome, CacheResult, TaskReport, TaskStatus,
};
use velnor_actions_contract::workflow::{Trust, WorkflowEvent};

pub(super) fn digest_fill(fill: char) -> Result<Digest, ContractError> {
    Digest::parse(&format!("b3-{}", fill.to_string().repeat(64)))
}

pub(super) fn fixture_job() -> Result<JobBindingV1, ContractError> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 42,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 123,
        run_attempt: 1,
        job_id: 456,
        job_key: "reader".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: "a".repeat(40),
        source_sha: "b".repeat(40),
        plan_digest: digest_fill('8')?,
    })
}

pub(super) fn fixture_generation() -> Result<GenerationId, ContractError> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: digest_fill('4')?,
        compatibility_digest: digest_fill('5')?,
        coverage_digest: digest_fill('6')?,
        repair_revision: 0,
    })
}

pub(super) fn fixture_context() -> Result<ExecutionContextV1, ContractError> {
    Ok(ExecutionContextV1 {
        schema: 1,
        job: fixture_job()?,
        generation: fixture_generation()?,
        requested_coverage_digest: digest_fill('7')?,
        source_binding_digest: digest_fill('9')?,
        workspace_root: "/home/runner/work/velnor-new/velnor-new".to_owned(),
        owned_root: "/home/runner/velnor-owned".to_owned(),
        cargo_home: "/home/runner/velnor-owned/cargo".to_owned(),
        rustup_home: "/home/runner/velnor-owned/rustup".to_owned(),
        tool_root: "/home/runner/velnor-owned/tools".to_owned(),
        transport_root: "/home/runner/work/velnor-new/transport".to_owned(),
        observation_root: "/home/runner/work/velnor-new/observations".to_owned(),
        lanes: vec![LaneContextV1 {
            lane_id: "lane-0".to_owned(),
            command_family: "build".to_owned(),
            variant_digest: digest_fill('a')?,
            target_dir: "target/debug".to_owned(),
            build_dir: None,
        }],
        compiler_adapters: vec![CompilerAdapterV1 {
            stable_path: "/opt/velnor/rustc".to_owned(),
            executable_identity: "rustc-1.99.1-x86_64".to_owned(),
        }],
        selected_driver: CacheDriver::Cargo,
        mbx_target_views: false,
        effective_cache_capability: CacheCapability::Read,
        trusted_default_writer: false,
        private_audience_digest: digest_fill('c')?,
    })
}

pub(super) fn fixture_source_binding() -> Result<SourceBindingV1, ContractError> {
    Ok(SourceBindingV1 {
        schema: 1,
        job: fixture_job()?,
        admitted_source_sha: "c".repeat(40),
        admitted_tree_id: "d".repeat(40),
        source_admission_digest: digest_fill('1')?,
        checkout_semantic_digest: digest_fill('2')?,
        declared_input_digest: digest_fill('3')?,
        git_semantics_digest: digest_fill('4')?,
        plan_digest: digest_fill('8')?,
    })
}

pub(super) fn fixture_unit(
    unit_id: &str,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<UnitResultV1, ContractError> {
    let unit = UnitResultV1 {
        unit_id: unit_id.to_owned(),
        recipe_id: digest_fill('1')?,
        task_identity: format!("task-identity-{unit_id}"),
        argv_digest: digest_fill('2')?,
        effective_input_digest: digest_fill('3')?,
        outcome,
        exit_code,
        started_at: "2026-10-26T00:00:00Z".to_owned(),
        finished_at: "2026-10-26T00:01:00Z".to_owned(),
        report_digest: digest_fill('0')?,
    };
    unit.validate()?;
    Ok(unit)
}

pub(super) fn fixture_ledger() -> Result<UnitLedgerV1, ContractError> {
    Ok(UnitLedgerV1 {
        schema: 1,
        job: fixture_job()?,
        group_digest: digest_fill('4')?,
        execution_branch: ExecutionBranch::Reader,
        selected_unit_ids: vec!["unit-a".to_owned(), "unit-b".to_owned()],
        ordered_results: vec![
            fixture_unit("unit-a", UnitOutcome::Executed, Some(0))?,
            fixture_unit("unit-b", UnitOutcome::CacheHit, None)?,
        ],
        observation_digest: digest_fill('5')?,
        source_binding_digest: digest_fill('2')?,
    })
}

pub(super) fn fixture_effective() -> Result<EffectiveJobBindingV1, ContractError> {
    Ok(EffectiveJobBindingV1 {
        job_key: "reader".to_owned(),
        job_id: 456,
        actual_attempt: 1,
        source_sha: "b".repeat(40),
        plan_digest: digest_fill('8')?,
        api_conclusion: ApiConclusion::Success,
        retained_without_rerun: true,
        ledger_digest: digest_fill('6')?,
    })
}

pub(super) fn fixture_report(
    status: TaskStatus,
    exit_code: i32,
) -> Result<TaskReport, ContractError> {
    let run_key = run_key_for_ci(123, 1);
    let task_id = task_id_for_internal("build", "dev")?;
    let matrix_id = matrix_id_for_task_group("web", task_id.as_str())?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    let task_digest = digest_fill('d')?;
    let task_report_id = task_report_id_for_task(&run_key, &matrix_key, task_digest.as_str())?;
    Ok(TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id,
        run_key,
        event: WorkflowEvent::Push,
        trust: Trust::Trusted,
        matrix_id,
        matrix_key,
        task_id,
        task_digest: task_digest.as_str().to_owned(),
        status,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "velnor-gc1-state-b3-0123456789abcdef".to_owned(),
            result: CacheResult::Hit,
            miss_reason: None,
        },
        platform_binding: PlatformBinding::unavailable(
            digest_fill('9')?.as_str(),
            PlatformRunnerEnvironment::Unknown,
            PlatformUnavailableReason::ObservationNotRecorded,
        )?,
        exit_code,
        duration_ms: Some(1000),
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    })
}

pub(super) fn fixture_unknown() -> Result<MeasuredU64, ContractError> {
    let unknown = MeasuredU64 {
        value: None,
        unknown_reason: Some(ObservationGapV1 {
            code: ObservationGapCode::UnknownNativeCoverage,
            detail: "native calls outside compiler adapters".to_owned(),
            affected_recipe_ids: Vec::new(),
            evidence_ref: "observation-session-1".to_owned(),
        }),
    };
    unknown.validate()?;
    Ok(unknown)
}
