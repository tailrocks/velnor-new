use std::collections::BTreeMap;

use crate::shell_step;

use velnor_actions_contract::{
    ExecutionMode, ImageConfig, ImageDockerfile, ImagePlatform, PermissionLevel, StepKind,
    config::{CheckExecutor, CheckPlatform, CheckRunner},
    workflow::CacheMode,
};

use super::{
    BUILD_IMAGE_NAME, BUILDKIT_IMAGE, CREATE_IMAGE_BUILDER_NAME, ImageBuildPolicy,
    REMOVE_IMAGE_BUILDER_NAME, UPLOAD_IMAGE_NAME, VERIFY_IMAGE_HOST_NAME, VERIFY_IMAGE_SOURCE_NAME,
    build_image_build_job, validate_image_build_jobs,
};

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn policy() -> ImageBuildPolicy {
    ImageBuildPolicy {
        image: ImageConfig {
            id: "eth-app".to_owned(),
            repository: "ghcr.io/chainargos/eth-app".to_owned(),
            dockerfile: ImageDockerfile::PerPlatform(BTreeMap::from([
                (ImagePlatform::LinuxAmd64, "svc/Dockerfile.amd64".to_owned()),
                (ImagePlatform::LinuxArm64, "svc/Dockerfile.arm64".to_owned()),
            ])),
            context: "svc".to_owned(),
            platforms: vec![ImagePlatform::LinuxAmd64, ImagePlatform::LinuxArm64],
            version_file: None,
            prebuilt: None,
            timeout_minutes: 30,
        },
        dockerfile_sha256: BTreeMap::from([
            (ImagePlatform::LinuxAmd64, "a".repeat(64)),
            (ImagePlatform::LinuxArm64, "b".repeat(64)),
        ]),
        prebuilt_steps: Vec::new(),
    }
}

fn script(job: &velnor_actions_contract::Job, name: &str) -> String {
    job.steps
        .iter()
        .find(|step| step.name == name)
        .and_then(|step| match &step.kind {
            StepKind::Shell { run, .. } => run.last().cloned(),
            _ => None,
        })
        .expect("shell step")
}

#[test]
fn job_is_native_unprivileged_and_ordered() {
    let policy = policy();
    assert_eq!(
        policy.job_ids(),
        ["image-eth-app-amd64", "image-eth-app-arm64"]
    );
    let job = build_image_build_job(&policy, ImagePlatform::LinuxArm64, CHECKOUT).expect("job");
    assert_eq!(job.runs_on, "ubuntu-26.04-arm");
    assert_eq!(job.timeout_minutes.minutes(), 30);
    assert!(job.environment.is_none());
    assert_eq!(job.needs, ["plan"]);
    assert_eq!(
        job.condition.as_deref(),
        Some("!contains(needs.plan.outputs.not_affected_jobs, ',image-eth-app-arm64,')")
    );
    let permissions = job.permissions.as_ref().expect("permissions");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    let names = job
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "Checkout",
            VERIFY_IMAGE_SOURCE_NAME,
            VERIFY_IMAGE_HOST_NAME,
            CREATE_IMAGE_BUILDER_NAME,
            BUILD_IMAGE_NAME,
            REMOVE_IMAGE_BUILDER_NAME,
            UPLOAD_IMAGE_NAME,
        ]
    );
    let remove = &job.steps[5];
    assert_eq!(remove.condition.as_deref(), Some("always()"));
}

#[test]
fn build_uses_the_platform_dockerfile_and_leaks_nothing_to_buildkit() {
    let policy = policy();
    let amd = build_image_build_job(&policy, ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    let build = script(&amd, BUILD_IMAGE_NAME);
    assert!(build.contains("--platform linux/amd64"));
    assert!(build.contains("--file 'svc/Dockerfile.amd64'"));
    assert!(build.contains("--sbom=true --provenance=mode=max"));
    assert!(build.contains("type=oci,dest=$out/image.tar"));
    assert!(build.ends_with("image.tar > image.tar.sha256"));
    for forbidden in [
        "--secret",
        "--ssh",
        "--build-arg",
        "type=gha",
        "--cache-to",
        "--cache-from",
        "ACTIONS_RUNTIME_TOKEN=",
    ] {
        assert!(
            !build.contains(forbidden),
            "The build script contains this forbidden option or value: {forbidden}."
        );
    }
    for step in &amd.steps {
        if let StepKind::Shell { env, .. } = &step.kind {
            assert!(env.values().all(String::is_empty), "{}: {env:?}", step.name);
        }
    }
    let create = script(&amd, CREATE_IMAGE_BUILDER_NAME);
    assert!(create.contains(&format!("image={BUILDKIT_IMAGE}")));
    assert!(create.contains("--driver docker-container"));
    let host = script(&amd, VERIFY_IMAGE_HOST_NAME);
    assert!(host.contains("grep -Fqx 'x86_64'"));
    let arm = build_image_build_job(&policy, ImagePlatform::LinuxArm64, CHECKOUT).expect("job");
    assert!(script(&arm, BUILD_IMAGE_NAME).contains("--file 'svc/Dockerfile.arm64'"));
    assert!(script(&arm, VERIFY_IMAGE_HOST_NAME).contains("grep -Fqx 'aarch64'"));
}

#[test]
fn source_guard_binds_dockerfile_digest_and_rejects_symlinks() {
    let job = build_image_build_job(&policy(), ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    let source = script(&job, VERIFY_IMAGE_SOURCE_NAME);
    assert!(source.contains("test ! -L 'svc'"));
    assert!(source.contains("test ! -L 'svc/Dockerfile.amd64'"));
    assert!(source.contains(&"a".repeat(64)));
    assert!(source.contains("sha256sum --check --status"));
}

#[test]
fn builder_is_task_owned_and_named_per_run_attempt_and_job() {
    let job = build_image_build_job(&policy(), ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    for name in [
        CREATE_IMAGE_BUILDER_NAME,
        BUILD_IMAGE_NAME,
        REMOVE_IMAGE_BUILDER_NAME,
    ] {
        assert!(script(&job, name).contains(
            "builder=\"velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-image-eth-app-amd64\""
        ));
    }
    assert!(script(&job, REMOVE_IMAGE_BUILDER_NAME).contains("buildx rm --force"));
}

#[test]
fn validation_rejects_missing_altered_and_bad_digest_jobs() {
    let policy = policy();
    let mut jobs = BTreeMap::new();
    assert!(
        validate_image_build_jobs(
            &jobs,
            &policy,
            CHECKOUT,
            &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
        )
        .is_err()
    );
    for platform in &policy.image.platforms {
        jobs.insert(
            policy.image.build_job_id(*platform),
            build_image_build_job(&policy, *platform, CHECKOUT).expect("job"),
        );
    }
    assert_eq!(
        validate_image_build_jobs(
            &jobs,
            &policy,
            CHECKOUT,
            &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
        )
        .expect("valid")
        .len(),
        2
    );
    let id = "image-eth-app-arm64";
    jobs.get_mut(id).expect("job").steps.pop();
    assert!(
        validate_image_build_jobs(
            &jobs,
            &policy,
            CHECKOUT,
            &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
        )
        .is_err()
    );
    let mut bad = policy;
    bad.dockerfile_sha256
        .insert(ImagePlatform::LinuxAmd64, "xyz".to_owned());
    assert!(build_image_build_job(&bad, ImagePlatform::LinuxAmd64, CHECKOUT).is_err());
}

fn amd64_policy() -> ImageBuildPolicy {
    let mut policy = policy();
    policy.image.platforms = vec![ImagePlatform::LinuxAmd64];
    policy.image.dockerfile = ImageDockerfile::PerPlatform(BTreeMap::from([(
        ImagePlatform::LinuxAmd64,
        "svc/Dockerfile.amd64".to_owned(),
    )]));
    policy
        .dockerfile_sha256
        .retain(|platform, _| *platform == ImagePlatform::LinuxAmd64);
    policy
}

fn routing(
    mode: velnor_actions_contract::ExecutionMode,
) -> crate::workflow_routing::WorkflowRoutingPolicy {
    let selector = velnor_actions_contract::ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("valid selector");
    crate::workflow_routing::test_policy(mode, selector)
}

#[test]
fn routing_validates_scaled_image_placement_and_preserves_nonplacement_contract() {
    let policy = amd64_policy();
    let id = policy.image.build_job_id(ImagePlatform::LinuxAmd64);
    let mut actual =
        build_image_build_job(&policy, ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    actual
        .display_name
        .push_str(super::super::SCALE_TASK_NAME_SUFFIX);
    let jobs = BTreeMap::from([(id.clone(), actual)]);
    validate_image_build_jobs(&jobs, &policy, CHECKOUT, &routing(ExecutionMode::ScaleSet))
        .expect("amd64 image routes to selected Scale Set");

    let mut altered = jobs.clone();
    altered
        .get_mut(&id)
        .expect("job")
        .display_name
        .push_str(" altered");
    assert!(
        validate_image_build_jobs(
            &altered,
            &policy,
            CHECKOUT,
            &routing(ExecutionMode::ScaleSet),
        )
        .is_err()
    );

    let mut cache_writer = jobs.clone();
    cache_writer.get_mut(&id).expect("job").cache_mode = Some(CacheMode::Write);
    assert!(
        validate_image_build_jobs(
            &cache_writer,
            &policy,
            CHECKOUT,
            &routing(ExecutionMode::ScaleSet),
        )
        .is_err()
    );

    let mut typed_runner = jobs.clone();
    typed_runner.get_mut(&id).expect("job").check_runner = Some(CheckRunner {
        label: "ubuntu-26.04".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    });
    assert!(
        validate_image_build_jobs(
            &typed_runner,
            &policy,
            CHECKOUT,
            &routing(ExecutionMode::ScaleSet),
        )
        .is_err()
    );

    let mut wrong_hosted_runner = jobs.clone();
    wrong_hosted_runner.get_mut(&id).expect("job").runs_on = "ubuntu-24.04".to_owned();
    assert!(
        validate_image_build_jobs(
            &wrong_hosted_runner,
            &policy,
            CHECKOUT,
            &routing(ExecutionMode::ScaleSet),
        )
        .is_err()
    );
}

#[test]
fn routing_rejects_arm64_scale_set_and_keeps_both_images_hosted() {
    let mut arm64 = policy();
    arm64.image.platforms = vec![ImagePlatform::LinuxArm64];
    arm64.image.dockerfile = ImageDockerfile::PerPlatform(BTreeMap::from([(
        ImagePlatform::LinuxArm64,
        "svc/Dockerfile.arm64".to_owned(),
    )]));
    arm64
        .dockerfile_sha256
        .retain(|platform, _| *platform == ImagePlatform::LinuxArm64);
    let arm_id = arm64.image.build_job_id(ImagePlatform::LinuxArm64);
    let arm_jobs = BTreeMap::from([(
        arm_id,
        build_image_build_job(&arm64, ImagePlatform::LinuxArm64, CHECKOUT).expect("job"),
    )]);
    assert!(
        validate_image_build_jobs(
            &arm_jobs,
            &arm64,
            CHECKOUT,
            &routing(ExecutionMode::ScaleSet),
        )
        .is_err()
    );

    let both = policy();
    let jobs = both
        .image
        .platforms
        .iter()
        .map(|platform| {
            (
                both.image.build_job_id(*platform),
                build_image_build_job(&both, *platform, CHECKOUT).expect("job"),
            )
        })
        .collect();
    validate_image_build_jobs(&jobs, &both, CHECKOUT, &routing(ExecutionMode::Both))
        .expect("Both keeps one hosted image job per platform");
}

#[test]
fn prebuilt_steps_sit_between_host_check_and_builder_and_need_the_plan() {
    let mut policy = policy();
    let prebuilt = shell_step(
        "Build release binary",
        vec!["true".to_owned()],
        BTreeMap::new(),
    )
    .expect("step");
    policy.prebuilt_steps = vec![prebuilt];
    assert!(policy.uses_pinned_tools());
    let job = build_image_build_job(&policy, ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    assert_eq!(job.needs, ["plan"]);
    let names = job
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>();
    let host = names
        .iter()
        .position(|n| *n == VERIFY_IMAGE_HOST_NAME)
        .expect("host");
    let build = names
        .iter()
        .position(|n| *n == "Build release binary")
        .expect("prebuilt");
    let builder = names
        .iter()
        .position(|n| *n == CREATE_IMAGE_BUILDER_NAME)
        .expect("builder");
    assert!(host < build && build < builder);
    let plain = self::policy();
    assert!(!plain.uses_pinned_tools());
    let job = build_image_build_job(&plain, ImagePlatform::LinuxAmd64, CHECKOUT).expect("job");
    assert_eq!(
        job.needs,
        ["plan"],
        "plain images still read the plan's gate"
    );
}
