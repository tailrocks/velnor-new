use std::collections::BTreeMap;

use velnor_actions_contract::{
    BuildTask, BuildTaskRunner, Job, JobTimeout, NativeImageCachePolicy, NativeImagePlatform,
    NativeImageTask, VerificationRunner, VerificationTask,
};

use super::{WorkflowTaskPolicy, extend_required_needs, validate_workflow_task_jobs};
use crate::{
    MiseSetup, VerificationTaskPolicy, build_verification_task_job,
    verification_jobs::{
        BuildTaskArtifact, BuildTaskPolicy, BuildTaskTool, NativeImageTaskPolicy,
        build_build_task_job, build_native_image_job,
    },
};

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
        version: "2026.10.5".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn verification_policy() -> VerificationTaskPolicy {
    VerificationTaskPolicy {
        task: VerificationTask {
            id: "native-format".to_owned(),
            mise_task: "desktop-format-check".to_owned(),
            runner: VerificationRunner::LinuxX64,
            timeout_minutes: 10,
        },
        runner_label: "ubuntu-26.04".to_owned(),
        scale_set_token: None,
        mise_setup: mise_setup(),
        selected_tools: Vec::new(),
        mise_config_sha256: None,
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
    }
}

fn build_policy() -> BuildTaskPolicy {
    BuildTaskPolicy {
        task: BuildTask {
            id: "native-desktop".to_owned(),
            mise_task: "desktop-ci".to_owned(),
            tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 120,
            cargo_build_jobs: 2,
            nextest_test_threads: 2,
        },
        runner_label: "macos-26".to_owned(),
        mise_setup: mise_setup(),
        mise_config_sha256: "b".repeat(64),
        mise_lock_sha256: "c".repeat(64),
        rust_toolchain_sha256: "d".repeat(64),
        selected_tools: vec![
            BuildTaskTool {
                key: "mr-boxington".to_owned(),
                version: "1.22.0".to_owned(),
                backend: "packslip:github.com/jdx/mr-boxington".to_owned(),
                os: Vec::new(),
                config_options: BTreeMap::new(),
                lock_options: BTreeMap::new(),
                artifact: Some(BuildTaskArtifact {
                    checksum: format!("sha256:{}", "e".repeat(64)),
                    url: "https://github.com/jdx/mr-boxington/releases/download/v1.22.0/mbx-aarch64-apple-darwin.tar.gz".to_owned(),
                    url_api: None,
                    signer: None,
                    provenance: None,
                }),
            },
            BuildTaskTool {
                key: "rust".to_owned(),
                version: "1.97.1".to_owned(),
                backend: "core:rust".to_owned(),
                os: Vec::new(),
                config_options: BTreeMap::new(),
                lock_options: BTreeMap::new(),
                artifact: None,
            },
        ],
    }
}

fn image_policy() -> NativeImageTaskPolicy {
    NativeImageTaskPolicy {
        task: NativeImageTask {
            id: "architect-arm64-image".to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
            timeout_minutes: 60,
            cache: NativeImageCachePolicy::TaskOwnedBuilder,
        },
        runner_label: "ubuntu-26.04-arm".to_owned(),
        source_sha256: "f".repeat(64),
    }
}

fn required_job() -> Job {
    Job {
        display_name: "Required".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    }
}

fn sorted_policies() -> Vec<WorkflowTaskPolicy> {
    vec![
        WorkflowTaskPolicy::NativeImage(image_policy()),
        WorkflowTaskPolicy::Build(build_policy()),
        WorkflowTaskPolicy::Verification(verification_policy()),
    ]
}

fn jobs_for(policies: &[WorkflowTaskPolicy]) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::new();
    for policy in policies {
        let (id, job) = match policy {
            WorkflowTaskPolicy::Verification(task) => (
                task.job_id(),
                build_verification_task_job(task, CHECKOUT).expect("verification job"),
            ),
            WorkflowTaskPolicy::Build(task) => (
                task.job_id(),
                build_build_task_job(task, CHECKOUT).expect("build job"),
            ),
            WorkflowTaskPolicy::NativeImage(task) => (
                task.job_id(),
                build_native_image_job(task, CHECKOUT).expect("image job"),
            ),
        };
        jobs.insert(id, job);
    }
    jobs.insert(crate::render::FINAL_JOB_ID.to_owned(), required_job());
    jobs
}

#[test]
fn mixed_variants_validate_and_join_required_fan_in() {
    let policies = sorted_policies();
    let mut jobs = jobs_for(&policies);
    let ids = validate_workflow_task_jobs(&jobs, &policies, CHECKOUT).expect("mixed task graph");
    assert_eq!(
        ids,
        [
            "task-architect-arm64-image",
            "task-native-desktop",
            "task-native-format",
        ]
    );
    extend_required_needs(&mut jobs, &ids).expect("required job exists");
    let needs = &jobs[crate::render::FINAL_JOB_ID].needs;
    assert!(needs.contains(&"task-architect-arm64-image".to_owned()));
    assert!(needs.contains(&"task-native-desktop".to_owned()));
    assert!(needs.contains(&"task-native-format".to_owned()));
    assert!(needs.contains(&"plan".to_owned()));
}

#[test]
fn undeclared_task_prefixed_jobs_fail_closed() {
    let policies = sorted_policies();
    let mut jobs = jobs_for(&policies);
    let extra = jobs["task-native-format"].clone();
    jobs.insert("task-extra".to_owned(), extra);
    let error = validate_workflow_task_jobs(&jobs, &policies, CHECKOUT)
        .expect_err("undeclared task job cannot pass");
    assert!(
        error.to_string().contains("undeclared_workflow_task_job"),
        "{error}"
    );
}

#[test]
fn unsorted_or_duplicate_policies_fail_closed() {
    let policies = sorted_policies();
    let jobs = jobs_for(&policies);
    let mut reversed = policies.clone();
    reversed.reverse();
    assert!(validate_workflow_task_jobs(&jobs, &reversed, CHECKOUT).is_err());
    let duplicated = vec![
        WorkflowTaskPolicy::Verification(verification_policy()),
        WorkflowTaskPolicy::Verification(verification_policy()),
    ];
    assert!(validate_workflow_task_jobs(&jobs, &duplicated, CHECKOUT).is_err());
}
