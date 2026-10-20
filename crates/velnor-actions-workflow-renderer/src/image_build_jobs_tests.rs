use std::collections::BTreeMap;

use velnor_actions_contract::{
    ImageConfig, ImageDockerfile, ImagePlatform, PermissionLevel, StepKind,
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
    assert!(job.needs.is_empty() && job.condition.is_none() && job.environment.is_none());
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
        assert!(!build.contains(forbidden), "{forbidden}");
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
    assert!(validate_image_build_jobs(&jobs, &policy, CHECKOUT).is_err());
    for platform in &policy.image.platforms {
        jobs.insert(
            policy.image.build_job_id(*platform),
            build_image_build_job(&policy, *platform, CHECKOUT).expect("job"),
        );
    }
    assert_eq!(
        validate_image_build_jobs(&jobs, &policy, CHECKOUT)
            .expect("valid")
            .len(),
        2
    );
    let id = "image-eth-app-arm64";
    jobs.get_mut(id).expect("job").steps.pop();
    assert!(validate_image_build_jobs(&jobs, &policy, CHECKOUT).is_err());
    let mut bad = policy;
    bad.dockerfile_sha256
        .insert(ImagePlatform::LinuxAmd64, "xyz".to_owned());
    assert!(build_image_build_job(&bad, ImagePlatform::LinuxAmd64, CHECKOUT).is_err());
}
