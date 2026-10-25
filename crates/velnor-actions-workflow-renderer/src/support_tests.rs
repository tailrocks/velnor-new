//! Consumer verification-merge cases.

use super::*;
use velnor_actions_contract::{
    GitHubEvidenceArtifactRefV1, GitHubEvidenceFetchSpecV1, GitHubEvidenceWorkflowRefV1,
    GitHubEvidenceWorkflowSourceRefV1, JobTimeout, PullRequestCachePolicy, Step, StepKind,
    StepRole, ValidatorKind, VelnorSupportWorkflow, canonical_json_str,
};

/// Render context with the given validator commands.
fn fixture_ctx(commands: Vec<ValidatorCommand>) -> RenderContext {
    let ctx = RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: std::collections::BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
        validator_commands: commands,
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    };
    ctx.validate().expect("fixture context is valid");
    ctx
}

/// Support set carrying exactly the given validators.
fn verify_support(validators: Vec<ValidatorKind>) -> VelnorSupportWorkflow {
    VelnorSupportWorkflow {
        validators,
        candidate_validation: false,
    }
}

/// Jobs map holding only the required-gate stub.
fn gate_jobs() -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        Job {
            services: Vec::new(),
            display_name: FINAL_DISPLAY_NAME.to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec![PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()],
            condition: Some(FINAL_CONDITION.to_owned()),
            permissions: None,
            environment: None,
            steps: Vec::new(),
            cache_mode: None,
        },
    );
    jobs
}

/// Fixed shell command for one shell-rendered validator.
fn shell_command(validator: ValidatorKind) -> ValidatorCommand {
    ValidatorCommand {
        validator,
        name: format!("Run {}", validator.display_name()),
        // Inert argv: no mise operations, so the install/exec closure
        // gate passes without a preparation vector.
        argv: vec!["tool".to_owned()],
        prepare_argv: Vec::new(),
    }
}

#[test]
fn consumer_empty_support_passes_through() {
    let ctx = fixture_ctx(Vec::new());
    let mut jobs = gate_jobs();
    merge_consumer_verify(&mut jobs, None, &ctx).expect("none passes");
    let empty = verify_support(Vec::new());
    merge_consumer_verify(&mut jobs, Some(&empty), &ctx).expect("empty passes");
    assert_eq!(jobs.len(), 1, "no jobs merged");
}

#[test]
fn consumer_verify_merges_and_gates() {
    let ctx = fixture_ctx(vec![shell_command(ValidatorKind::Markdownlint)]);
    let mut jobs = gate_jobs();
    let support = verify_support(vec![ValidatorKind::Alint, ValidatorKind::Markdownlint]);
    merge_consumer_verify(&mut jobs, Some(&support), &ctx).expect("verify merges");
    assert!(jobs.contains_key("alint"), "alint merged");
    assert!(jobs.contains_key("markdownlint"), "markdownlint merged");
    let needs = &jobs.get(FINAL_JOB_ID).expect("gate").needs;
    assert!(needs.contains(&"alint".to_owned()), "gate needs alint");
    assert!(
        needs.contains(&"markdownlint".to_owned()),
        "gate needs markdownlint"
    );
}

#[test]
fn consumer_rejects_velnor_only_candidate_and_duplicates() {
    let ctx = fixture_ctx(Vec::new());
    let denied = verify_support(vec![ValidatorKind::CargoDeny]);
    let err = merge_consumer_verify(&mut gate_jobs(), Some(&denied), &ctx)
        .expect_err("cargo-deny rejected");
    assert!(
        err.to_string()
            .contains("consumer_validator_rejected:cargo-deny"),
        "{err}"
    );
    let lint = verify_support(vec![ValidatorKind::Jactionlint]);
    let err = merge_consumer_verify(&mut gate_jobs(), Some(&lint), &ctx)
        .expect_err("actionlint rejected");
    assert!(
        err.to_string().contains("consumer_validator_rejected"),
        "{err}"
    );
    let candidate = VelnorSupportWorkflow {
        validators: Vec::new(),
        candidate_validation: true,
    };
    let err = merge_consumer_verify(&mut gate_jobs(), Some(&candidate), &ctx)
        .expect_err("candidate rejected");
    assert!(err.to_string().contains("support_ir_rejected"), "{err}");
    let dupe = verify_support(vec![ValidatorKind::Alint, ValidatorKind::Alint]);
    let err =
        merge_consumer_verify(&mut gate_jobs(), Some(&dupe), &ctx).expect_err("dupe rejected");
    assert!(err.to_string().contains("duplicate_validator"), "{err}");
}

#[test]
fn consumer_ir_still_rejects_velnor_ids() {
    let ctx = fixture_ctx(Vec::new());
    let mut jobs = gate_jobs();
    jobs.insert(
        "alint".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Alint".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: Vec::new(),
            cache_mode: None,
        },
    );
    let err = merge_consumer_verify(&mut jobs, None, &ctx).expect_err("IR alint rejected");
    assert!(err.to_string().contains("forbidden_job:alint"), "{err}");
}

fn evidence_fetch_spec() -> String {
    let spec = GitHubEvidenceFetchSpecV1 {
        schema: velnor_actions_contract::GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
        collector_suite_id: "collector".to_owned(),
        collector_workflow_path: ".github/workflows/nightly.yml".to_owned(),
        default_branch: "main".to_owned(),
        window_days: 7,
        workflow_sources: vec![
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::PrimaryCi,
                path: ".github/workflows/ci.yml".to_owned(),
                job_id: None,
                job_name: None,
            },
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::TaskSuite {
                    suite_id: "producer".to_owned(),
                },
                path: ".github/workflows/producer.yml".to_owned(),
                job_id: Some("mise-task-producer".to_owned()),
                job_name: Some("Mise task producer".to_owned()),
            },
        ],
        artifact_sources: vec![GitHubEvidenceArtifactRefV1 {
            suite_id: "producer".to_owned(),
            artifact_name: "ledger".to_owned(),
            workflow_path: ".github/workflows/producer.yml".to_owned(),
            job_id: "mise-task-producer".to_owned(),
            job_name: "Mise task producer".to_owned(),
        }],
    };
    canonical_json_str(&spec).expect("canonical spec")
}

fn evidence_job(id: &str) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        id.to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Evidence fetch".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![Step {
                name: "Fetch evidence".to_owned(),
                id: None,
                role: Some(StepRole::GitHubEvidenceFetch),
                condition: None,
                parallel: false,
                kind: StepKind::Internal {
                    operation: velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION.to_owned(),
                    env: BTreeMap::from([
                        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
                        (
                            velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV.to_owned(),
                            evidence_fetch_spec(),
                        ),
                    ]),
                },
            }],
            cache_mode: None,
        },
    );
    jobs
}

#[test]
fn evidence_fetch_token_is_bound_to_its_exact_collector_job() {
    assert!(tokens::check_token_hygiene(&evidence_job("github-evidence-fetch-collector")).is_ok());
    assert!(tokens::check_token_hygiene(&evidence_job("github-evidence-fetch-unrelated")).is_err());
}

fn native_token_job(job_id: &str, kind: StepKind) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        job_id.to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Native check".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![Step {
                name: "User-controlled label is irrelevant".to_owned(),
                id: None,
                role: None,
                condition: None,
                parallel: false,
                kind,
            }],
            cache_mode: None,
        },
    );
    jobs
}

#[test]
fn native_token_scope_requires_the_typed_sdk_dispatch() {
    use velnor_actions_contract::config::NativeHomebrewOperation;

    let typed = StepKind::NativeCheckExecution {
        check_id: "tap-tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        task_id: "internal/native-check/tap-tree".to_owned(),
        sdk_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    assert!(tokens::check_token_hygiene(&native_token_job("check-tap-tree", typed)).is_ok());
    assert!(
        tokens::check_token_hygiene(&native_token_job(
            "check-tap-tree",
            StepKind::NativeCheckExecution {
                check_id: "tap-tree".to_owned(),
                operation: NativeHomebrewOperation::TapTree,
                task_id: "wrong-task".to_owned(),
                sdk_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        ))
        .is_err()
    );
    assert!(
        tokens::check_token_hygiene(&native_token_job(
            "check-tap-tree",
            StepKind::NativeCheckExecution {
                check_id: "tap-tree".to_owned(),
                operation: NativeHomebrewOperation::TapTree,
                task_id: "internal/native-check/tap-tree".to_owned(),
                sdk_version: "0.0.0".to_owned(),
            },
        ))
        .is_err()
    );

    let spoofed_env = BTreeMap::from([
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            "VELNOR_INTERNAL_OP".to_owned(),
            "execute-native-homebrew-check-v1".to_owned(),
        ),
    ]);
    let spoofed = StepKind::Shell {
        run: vec!["velnor-actions-0.1.0".to_owned()],
        env: spoofed_env,
    };
    assert!(tokens::check_token_hygiene(&native_token_job("check-tap-tree", spoofed)).is_err());
}
