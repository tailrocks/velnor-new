//! Contract config, manifest, plan, report, and workflow cases.
use crate::impl_contract_ids::{MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ActionPin, BaselineProof, BaselineStatus, CacheLayer, CacheOutcome, CacheResult,
    CandidateReport, CandidateStatus, Concurrency, ContractError, FinalCounts, FinalReport,
    FinalStatus, GeneratorBinary, GeneratorLock, GeneratorValidation, Job, LockedGenerator,
    MatrixReport, MatrixStatus, MiseBootstrap, NotSelectedReason, ObligationDecision, Permissions,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner,
    ReleaseManifest, RequiredJobResult, RunnerSelection, Step, StepKind, TargetRecord, TaskReport,
    TaskStatus, Trigger, Trust, WorkflowEvent, WorkflowIr, WorkflowPolicy,
    artifact_id_for_candidate, artifact_id_for_matrix, artifact_id_for_plan,
    candidate_report_id_for_run, digest_b3, final_report_id_for_run, plan_id_for_run,
    run_key_for_ci, task_report_id_for_task, validate_candidate_report_id,
    validate_final_report_id,
};

#[test]
fn config_validation_reports_key_paths() {
    use velnor_actions_contract::{
        DiscoveryConfig, ResourcesConfig, RustConfiguration, RustStackConfig, StacksConfig,
        TestShardingConfig, VelnorConfig, WorkflowConfig,
    };
    let valid = VelnorConfig {
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
        },
        resources: ResourcesConfig {
            compiler_process_budget: 2,
            test_process_budget: 2,
        },
        test_sharding: TestShardingConfig {
            default_shards: 1,
            by_manifest: BTreeMap::from([("crates/large/Cargo.toml".to_owned(), 2)]),
        },
        stacks: StacksConfig {
            ignore: vec![],
            rust: Some(RustStackConfig {
                configurations: vec![RustConfiguration {
                    name: "default".to_owned(),
                    features: vec!["default".to_owned()],
                    target: "host".to_owned(),
                }],
            }),
        },
        discovery: DiscoveryConfig {
            exclude: vec!["vendor/**".to_owned()],
        },
    };
    assert_eq!(valid.validate(".velnor/config.toml"), Ok(()));
    let support = WorkflowPolicy::ConsumerV1.support_workflow(GeneratorValidation::Bootstrap);
    assert!(support.policy_jobs.is_empty() && !support.candidate_validation);
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    assert_eq!(support.policy_jobs.len(), 2);
    assert!(support.candidate_validation);
    let mut bad = valid.clone();
    bad.schema = 2;
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid.clone();
    bad.stacks.ignore = vec!["rust".to_owned(), "bogus".to_owned()];
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid.clone();
    bad.discovery.exclude = vec!["/absolute/**".to_owned()];
    assert!(bad.validate("cfg").is_err());
    let mut bad = valid;
    bad.workflow.runner_label = Some("ubuntu-latest".to_owned());
    assert!(bad.validate("cfg").is_err());
}

#[test]
fn manifest_schemas_validate_and_lookup_targets() -> Result<(), ContractError> {
    let sha = "ab".repeat(32);
    let manifest = ReleaseManifest {
        schema: 1,
        version: "0.1.0".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        targets: vec![TargetRecord {
            target: "x86_64-unknown-linux-gnu".to_owned(),
            artifact: "https://example.com/velnor-actions-0.1.0-x86_64".to_owned(),
            sha256: sha.clone(),
        }],
    };
    manifest.validate("release.toml")?;
    assert!(
        manifest
            .record_for_target("x86_64-unknown-linux-gnu")
            .is_some()
    );
    assert!(manifest.record_for_target("other").is_none());
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.com/velnor-actions-0.1.0".to_owned(),
                sha256: sha,
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2025.1.0".to_owned(),
            artifact: "https://example.com/mise-2025.1.0".to_owned(),
            sha256: "cd".repeat(32),
        },
        actions: vec![ActionPin {
            name: "actions/checkout".to_owned(),
            version: "v4.2.2".to_owned(),
            sha: "ab".repeat(20),
            reviewed: "2026-01-15".to_owned(),
        }],
    };
    lock.validate("generator.lock")?;
    assert!(lock.binary_for_target("x86_64-unknown-linux-gnu").is_some());
    let mut bad = lock.clone();
    bad.actions[0].sha = "xyz".to_owned();
    assert!(bad.validate("generator.lock").is_err());
    Ok(())
}

#[test]
fn plan_validates_sorting_and_matrix() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(3, 1);
    let entry = sample_entry(&run_key)?;
    let plan = Plan {
        schema: 1,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        base: None,
        head: "ab".repeat(20),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline {
            status: BaselineStatus::Unavailable,
            base_commit: None,
            run_id: None,
            artifact_id: None,
            artifact_name: None,
            manifest_digest: None,
            reason: Some("no_entry".to_owned()),
        },
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
    };
    plan.validate()?;
    let mut bad = plan.clone();
    bad.task_ids = vec![TASK.to_owned(), "aaa".to_owned()];
    assert!(bad.validate().is_err());
    let covered = PlanObligation {
        decision: ObligationDecision::CoveredByTrustedBaseline,
        baseline_proof: Some(BaselineProof {
            source_commit: "ab".repeat(20),
            run_id: 99,
            artifact_id: 4242,
            artifact_name: artifact_id_for_plan(&run_key)?,
            manifest_digest: digest_b3(b"manifest"),
        }),
        ..plan.obligations[0].clone()
    };
    let mut plan2 = plan.clone();
    plan2.obligations = vec![covered];
    plan2.validate()?;
    Ok(())
}

#[test]
fn task_and_matrix_reports_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(11, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let report = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?,
        run_key: run_key.clone(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: TASK.to_owned(),
        task_digest,
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "velnor-v1-task-pr-x".to_owned(),
            result: CacheResult::Miss,
            miss_reason: Some("no_entry".to_owned()),
        },
        exit_code: 0,
        duration_ms: 12,
        outputs: vec![],
    };
    report.validate()?;
    let skipped = TaskReport {
        status: TaskStatus::NotSelected,
        not_selected_reason: Some(NotSelectedReason::UpstreamFailed),
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "k".to_owned(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        ..report.clone()
    };
    skipped.validate()?;
    let matrix = MatrixReport {
        schema: 1,
        report_id: entry.report_id.clone(),
        run_key,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![TASK.to_owned()],
        task_report_ids: vec![report.task_report_id.clone()],
        tasks: vec![velnor_actions_contract::MatrixTaskEntry {
            task_report_id: report.task_report_id.clone(),
            task_id: TASK.to_owned(),
            status: TaskStatus::Executed,
            exit_code: 0,
        }],
        selected: 1,
        reused: 0,
        executed: 1,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    matrix.validate()?;
    Ok(())
}

#[test]
fn final_and_candidate_reports_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(5, 3);
    let entry = sample_entry(&run_key)?;
    let report_id = final_report_id_for_run(&run_key)?;
    validate_final_report_id(&report_id)?;
    let final_report = FinalReport {
        schema: 1,
        report_id,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        expected_report_ids: vec![entry.report_id.clone()],
        downloaded_artifact_ids: vec![
            artifact_id_for_matrix(&run_key, &entry.matrix_key)?,
            artifact_id_for_plan(&run_key)?,
        ],
        required_job_results: vec![RequiredJobResult {
            job_id: "velnor-plan".to_owned(),
            conclusion: "success".to_owned(),
        }],
        status: FinalStatus::Passed,
        counts: FinalCounts {
            selected: 1,
            reused: 0,
            executed: 1,
            empty_partition: 0,
            covered: 0,
            failed: 0,
            cancelled: 0,
            blocked: 0,
            not_run: 0,
        },
    };
    final_report.validate()?;
    assert_eq!(
        final_report.artifact_id()?,
        format!("velnor-final-{run_key}")
    );
    let candidate_id = candidate_report_id_for_run(&run_key, "x86_64-unknown-linux-gnu")?;
    validate_candidate_report_id(&candidate_id)?;
    let candidate = CandidateReport {
        schema: 1,
        report_id: candidate_id,
        run_key: run_key.clone(),
        source_commit: "ab".repeat(20),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        artifact_sha256: "cd".repeat(32),
        generator_version: "0.1.0".to_owned(),
        status: CandidateStatus::Passed,
        checks: vec!["generate-check".to_owned()],
    };
    candidate.validate()?;
    assert_eq!(
        candidate.artifact_id()?,
        artifact_id_for_candidate(&run_key, &candidate.target)?
    );
    Ok(())
}

#[test]
fn workflow_ir_validates_pins_and_refs() -> Result<(), ContractError> {
    let job = Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        needs: vec![],
        condition: None,
        steps: vec![Step {
            name: "Checkout".to_owned(),
            kind: StepKind::Shell {
                run: vec!["mise".to_owned(), "install".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    };
    let ir = WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
        },
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
        },
        concurrency: Concurrency {
            group: "velnor-ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs: BTreeMap::from([("velnor-plan".to_owned(), job)]),
    };
    ir.validate()?;
    let mut bad = ir.clone();
    if let Some(job) = bad.jobs.get_mut("velnor-plan") {
        job.runs_on = "ubuntu-latest".to_owned();
    }
    assert!(bad.validate().is_err());
    Ok(())
}
