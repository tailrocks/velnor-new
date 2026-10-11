//! Consumer verification-merge cases.

use super::*;
use velnor_actions_contract::{
    JobTimeout, PullRequestCachePolicy, ValidatorKind, VelnorSupportWorkflow,
};

/// Render context with the given validator commands.
fn fixture_ctx(commands: Vec<ValidatorCommand>) -> RenderContext {
    let ctx = RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
        validator_commands: commands,
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
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
            display_name: FINAL_DISPLAY_NAME.to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: vec![PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()],
            condition: Some(FINAL_CONDITION.to_owned()),
            permissions: None,
            environment: None,
            steps: Vec::new(),
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
    let lint = verify_support(vec![ValidatorKind::Actionlint]);
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
            display_name: "Alint".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: Vec::new(),
        },
    );
    let err = merge_consumer_verify(&mut jobs, None, &ctx).expect_err("IR alint rejected");
    assert!(err.to_string().contains("forbidden_job:alint"), "{err}");
}
