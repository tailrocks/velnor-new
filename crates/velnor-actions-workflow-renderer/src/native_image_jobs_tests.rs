use std::collections::BTreeMap;

use velnor_actions_contract::{
    NativeImageCachePolicy, NativeImagePlatform, NativeImageTask, PermissionLevel, StepKind,
};

use super::{
    NativeImageTaskPolicy, RUN_NATIVE_IMAGE_TASK_NAME, VERIFY_NATIVE_IMAGE_HOST_NAME,
    VERIFY_NATIVE_IMAGE_SOURCE_NAME, build_native_image_job, validate_native_image_jobs,
};

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn policy() -> NativeImageTaskPolicy {
    NativeImageTaskPolicy {
        task: NativeImageTask {
            id: "architect-arm64-image".to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
            timeout_minutes: 60,
            cache: NativeImageCachePolicy::TaskOwnedBuilder,
        },
        runner_label: "ubuntu-26.04-arm".to_owned(),
        source_sha256: "e".repeat(64),
    }
}

fn shell_runs(job: &velnor_actions_contract::Job) -> Vec<&Vec<String>> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Shell { run, .. } => Some(run),
            _ => None,
        })
        .collect()
}

#[test]
fn native_image_job_is_hosted_least_privilege_and_source_bound() {
    let policy = policy();
    let job = build_native_image_job(&policy, CHECKOUT).expect("native image job");
    assert_eq!(policy.job_id(), "task-architect-arm64-image");
    assert_eq!(job.runs_on, "ubuntu-26.04-arm");
    assert_eq!(job.timeout_minutes.minutes(), 60);
    assert!(job.needs.is_empty());
    assert!(job.condition.is_none());
    assert!(job.environment.is_none());
    let permissions = job.permissions.as_ref().expect("explicit permissions");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(job.steps.len(), 4);
    assert!(matches!(&job.steps[0].kind, StepKind::Action { .. }));
    assert_eq!(job.steps[1].name, VERIFY_NATIVE_IMAGE_SOURCE_NAME);
    assert_eq!(job.steps[2].name, VERIFY_NATIVE_IMAGE_HOST_NAME);
    assert_eq!(job.steps[3].name, RUN_NATIVE_IMAGE_TASK_NAME);

    let runs = shell_runs(&job);
    assert_eq!(runs.len(), 3);
    let source = runs[0].join(" ");
    assert!(source.contains("test ! -L 'maintained-image-build'"));
    assert!(source.contains("test -f 'maintained-image-build/arm64-image-validation.sh'"));
    assert!(source.contains(&"e".repeat(64)));
    assert!(source.contains("sha256sum --check --status"));
    let host = runs[1].join(" ");
    assert!(host.contains("grep -Fqx 'Linux'"));
    assert!(host.contains("grep -Fqx 'aarch64'"));
    assert!(host.contains("linux/aarch64"));
    assert!(host.contains("buildx version"));
    let run = runs[2].join(" ");
    assert!(run.contains("maintained-image-build/arm64-image-validation.sh"));
    assert!(run.contains("linux/arm64"));
    assert!(run.contains("DOCKER_HOST=unix:///var/run/docker.sock"));

    for run in runs {
        for arg in run {
            assert!(!arg.contains('\n'), "single-line script: {arg}");
            assert!(!arg.contains("$("), "no substitution: {arg}");
            assert!(!arg.contains('`'), "no backticks: {arg}");
        }
    }
}

#[test]
fn declared_image_jobs_are_exact_sorted_and_unique() {
    let declared = policy();
    let id = declared.job_id();
    let job = build_native_image_job(&declared, CHECKOUT).expect("native image job");
    let jobs = BTreeMap::from([(id.clone(), job)]);
    assert_eq!(
        validate_native_image_jobs(&jobs, std::slice::from_ref(&declared), CHECKOUT)
            .expect("declared image job"),
        [id]
    );

    let mut wrong_runner = policy();
    wrong_runner.runner_label = "ubuntu-26.04".to_owned();
    assert!(build_native_image_job(&wrong_runner, CHECKOUT).is_err());

    let mut bad_digest = policy();
    bad_digest.source_sha256 = "not-a-digest".to_owned();
    assert!(build_native_image_job(&bad_digest, CHECKOUT).is_err());
}
