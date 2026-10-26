use std::{collections::BTreeMap, error::Error, io};

use velnor_actions_contract::{GeneratorBinary, GeneratorLock, Job, Step, StepKind, StepRole};
use velnor_actions_orchestrator::{GenerationPreparation, OrchestratorError, finalized_jobs};
use velnor_actions_workflow_renderer::{
    ASSET_SHA_ENV, ASSET_URL_ENV, FINAL_JOB_ID, PLAN_JOB_ID, RELEASE_COMMIT_ENV,
    STAGED_BINARY_PREFIX,
};

pub(super) const BOOTSTRAP_CONFIG: &str = r#"schema = 2
[workflow]
policy = "velnor-repository-v1"
default_branch = "main"
generator_validation = "bootstrap"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
[[native_checks]]
id = "fixture-native-tree"
operation = "tap_tree"
[native_checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
"#;

pub(super) fn assert_bootstrap_authority(
    preparation: &GenerationPreparation,
    jobs: &BTreeMap<String, Job>,
    lock: &GeneratorLock,
) -> Result<(), Box<dyn Error>> {
    let helper_version = lock.generator.version.as_str();
    assert_generator_versions(preparation, helper_version);
    assert_plan_authority(jobs, lock)?;
    assert_source_free_boundary(jobs)?;
    assert_helper_versions(jobs, helper_version);
    assert_native_check(jobs, lock, helper_version)
}

pub(super) fn assert_plan_checkout_pin_is_required(
    preparation: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let mut changed = preparation.clone();
    let plan = changed
        .workflow
        .ir
        .jobs
        .get_mut(PLAN_JOB_ID)
        .ok_or_else(|| io::Error::other("bootstrap workflow omitted Plan"))?;
    let checkout = plan
        .steps
        .iter_mut()
        .find(|step| step.role == Some(StepRole::Checkout))
        .ok_or_else(|| io::Error::other("Plan omitted its trusted checkout"))?;
    let StepKind::Action { uses, .. } = &mut checkout.kind else {
        return Err(io::Error::other("Plan checkout is not an action").into());
    };
    "actions/checkout@0000000000000000000000000000000000000000".clone_into(uses);
    let error = finalized_jobs(&changed).err().ok_or_else(|| {
        io::Error::other("Plan must reject a checkout action with a different pin")
    })?;
    assert!(matches!(
        error,
        OrchestratorError::Render { problem }
            if problem == "invalid workflow: trusted_plan_candidate_execution"
    ));
    Ok(())
}

fn assert_generator_versions(preparation: &GenerationPreparation, helper_version: &str) {
    assert_eq!(
        preparation.workflow.context.generator_version,
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        preparation.workflow.context.report_helper_version,
        helper_version
    );
    assert_ne!(helper_version, env!("CARGO_PKG_VERSION"));
}

fn assert_plan_authority(
    jobs: &BTreeMap<String, Job>,
    lock: &velnor_actions_contract::GeneratorLock,
) -> Result<(), Box<dyn Error>> {
    let binary = plan_binary(&lock.generator.binaries)?;
    let plan = jobs
        .get(PLAN_JOB_ID)
        .ok_or_else(|| io::Error::other("bootstrap workflow omitted Plan"))?;
    let acquire_steps = plan
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::AcquireVelnor))
        .collect::<Vec<_>>();
    assert_eq!(acquire_steps.len(), 1);
    let (acquire_index, acquire_step) = acquire_steps[0];
    assert_lock_acquisition(acquire_step, lock, binary)?;
    let internal_operations = plan
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| matches!(&step.kind, StepKind::Internal { .. }))
        .collect::<Vec<_>>();
    assert_ne!(internal_operations.len(), 0);
    assert!(
        internal_operations
            .iter()
            .all(|(index, _)| acquire_index < *index)
    );
    assert!(
        plan.steps
            .iter()
            .any(|step| step.role == Some(StepRole::PlanProducer))
    );
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::CheckGenerated))
    );
    Ok(())
}

fn assert_source_free_boundary(jobs: &BTreeMap<String, Job>) -> Result<(), Box<dyn Error>> {
    let plan = jobs
        .get(PLAN_JOB_ID)
        .ok_or_else(|| io::Error::other("bootstrap workflow omitted Plan"))?;
    let required = jobs
        .get(FINAL_JOB_ID)
        .ok_or_else(|| io::Error::other("bootstrap workflow omitted Required"))?;
    for step in plan.steps.iter().chain(required.steps.iter()) {
        if let StepKind::Action { uses, .. } = &step.kind {
            assert!(
                !uses.starts_with("./"),
                "local candidate or cache action appears before trusted authority: {uses}"
            );
        }
    }
    Ok(())
}

fn assert_helper_versions(jobs: &BTreeMap<String, Job>, helper_version: &str) {
    let mut task_executions = 0;
    let mut native_check_executions = 0;
    for job in jobs.values() {
        for step in &job.steps {
            assert!(!has_candidate_preseed_role(step.role));
            match &step.kind {
                StepKind::TaskExecution {
                    report_helper_version,
                    ..
                } => {
                    task_executions += 1;
                    assert_eq!(report_helper_version, helper_version);
                }
                StepKind::NativeCheckExecution { sdk_version, .. } => {
                    native_check_executions += 1;
                    assert_eq!(sdk_version, helper_version);
                }
                _ => {}
            }
        }
    }
    assert!(task_executions > 0, "fixture has no crate task to verify");
    assert_eq!(native_check_executions, 1);
}

fn assert_native_check(
    jobs: &BTreeMap<String, Job>,
    lock: &velnor_actions_contract::GeneratorLock,
    helper_version: &str,
) -> Result<(), Box<dyn Error>> {
    let check = jobs
        .get("check-fixture-native-tree")
        .ok_or_else(|| io::Error::other("bootstrap workflow omitted its native check"))?;
    let executions = check
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| matches!(&step.kind, StepKind::NativeCheckExecution { .. }))
        .collect::<Vec<_>>();
    assert_eq!(executions.len(), 1);
    let (execution_index, execution) = executions[0];
    let StepKind::NativeCheckExecution {
        check_id,
        operation,
        task_id,
        sdk_version,
    } = &execution.kind
    else {
        return Err(io::Error::other("native check job has an invalid step").into());
    };
    assert_eq!(check_id, "fixture-native-tree");
    assert_eq!(
        *operation,
        velnor_actions_contract::config::NativeHomebrewOperation::TapTree
    );
    assert_eq!(task_id, "internal/native-check/fixture-native-tree");
    assert_eq!(sdk_version, helper_version);

    let binary = plan_binary(&lock.generator.binaries)?;
    let acquisitions = check
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::AcquireVelnor))
        .collect::<Vec<_>>();
    assert_eq!(acquisitions.len(), 1);
    let (acquire_index, acquire) = acquisitions[0];
    assert!(acquire_index < execution_index);
    assert_lock_acquisition(acquire, lock, binary)
}

fn assert_lock_acquisition(
    step: &Step,
    lock: &velnor_actions_contract::GeneratorLock,
    binary: &GeneratorBinary,
) -> Result<(), Box<dyn Error>> {
    let StepKind::Shell { env, run } = &step.kind else {
        return Err(io::Error::other("Velnor acquisition is not a shell step").into());
    };
    assert_eq!(
        env.get(ASSET_URL_ENV).map(String::as_str),
        Some(binary.artifact.as_str())
    );
    assert_eq!(
        env.get(ASSET_SHA_ENV).map(String::as_str),
        Some(binary.sha256.as_str())
    );
    assert_eq!(
        env.get(RELEASE_COMMIT_ENV).map(String::as_str),
        Some(lock.generator.commit.as_str())
    );
    let staged_binary = format!("{STAGED_BINARY_PREFIX}{}", lock.generator.version);
    assert!(run.iter().any(|command| command.contains(&staged_binary)));
    Ok(())
}

fn plan_binary(binaries: &[GeneratorBinary]) -> Result<&GeneratorBinary, Box<dyn Error>> {
    binaries
        .iter()
        .find(|binary| binary.target == "x86_64-unknown-linux-gnu")
        .ok_or_else(|| io::Error::other("published generator lock omitted the Plan binary").into())
}

fn has_candidate_preseed_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreseedBuild
                | StepRole::PreseedManifest
                | StepRole::PreseedVerifyBuild
                | StepRole::PreseedUpload
                | StepRole::PreseedDownload
                | StepRole::PreseedVerifyManifest
                | StepRole::PreseedStage
        )
    )
}

pub(super) fn assert_symlink_bootstrap(
    root: &std::path::Path,
    lock: &GeneratorLock,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(
        root.join(".velnor/config.toml"),
        format!("{BOOTSTRAP_CONFIG}\n[agents]\nclaude_md = \"symlink\"\n"),
    )?;
    let preparation = velnor_actions_orchestrator::prepare(root)?;
    assert!(
        preparation.lock_audit_blocking.is_empty(),
        "symlink producer preparation must pass lock audit: {:?}",
        preparation.lock_audit_blocking
    );
    let jobs = finalized_jobs(&preparation)?;
    assert_bootstrap_authority(&preparation, &jobs, lock)
}
