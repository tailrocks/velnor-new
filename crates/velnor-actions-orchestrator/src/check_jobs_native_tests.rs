use super::super::check_jobs::build_check_jobs;
use crate::discover::Discovery;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck, NativeHomebrewOperation,
};
use velnor_actions_contract::{
    SUPPORTED_TARGETS, StepKind, StepRole, WorkflowPolicy, asset_filename,
};
use velnor_actions_mise::ToolCatalog;

fn runner(label: &str, platform: CheckPlatform) -> CheckRunner {
    CheckRunner {
        label: label.to_owned(),
        platform,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

fn manifest() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = SUPPORTED_TARGETS
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let digest = char::from(b'a' + u8::try_from(index).expect("target index"))
                .to_string()
                .repeat(64);
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/{}\",\"sha256\":\"{digest}\"}}",
                asset_filename(version, target)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "f".repeat(40)
    )
}

fn native_check(id: &str, runner: CheckRunner) -> NativeHomebrewCheck {
    NativeHomebrewCheck {
        id: id.to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner,
        timeout_minutes: 5,
    }
}

#[test]
fn consumer_native_jobs_acquire_matching_manifest_target_after_checkout() {
    let expected = [
        (
            "linux",
            "ubuntu-26.04",
            CheckPlatform::LinuxX64,
            "x86_64-unknown-linux-gnu",
            "a",
        ),
        (
            "mac-arm",
            "macos-15",
            CheckPlatform::MacosArm64,
            "aarch64-apple-darwin",
            "b",
        ),
        (
            "mac-intel",
            "macos-15-intel",
            CheckPlatform::MacosX64,
            "x86_64-apple-darwin",
            "c",
        ),
    ];
    let discovery = Discovery {
        native_checks: expected
            .iter()
            .map(|(id, label, platform, _, _)| native_check(id, runner(label, *platform)))
            .collect(),
        consumer_manifest_json: Some(manifest()),
        ..Discovery::default()
    };

    let jobs = build_check_jobs(
        WorkflowPolicy::ConsumerV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("consumer native jobs acquire the published SDK");
    assert_eq!(jobs.len(), expected.len());

    for ((id, job), (check_id, label, _, target, digest_prefix)) in jobs.iter().zip(expected) {
        assert_eq!(id, &format!("check-{check_id}"));
        assert_eq!(job.runs_on, label);
        assert_eq!(job.steps[0].role, Some(StepRole::Checkout));
        let acquire = &job.steps[1];
        assert_eq!(acquire.role, Some(StepRole::AcquireVelnor));
        let StepKind::Shell { env, .. } = &acquire.kind else {
            panic!("Acquire Velnor shell step");
        };
        let version = env!("CARGO_PKG_VERSION");
        let expected_url = format!(
            "https://github.com/tailrocks/velnor-new/releases/download/v{version}/{}",
            asset_filename(version, target)
        );
        let expected_digest = digest_prefix.repeat(64);
        assert_eq!(
            env.get(velnor_actions_workflow_renderer::ASSET_URL_ENV),
            Some(&expected_url)
        );
        assert_eq!(
            env.get(velnor_actions_workflow_renderer::ASSET_SHA_ENV),
            Some(&expected_digest)
        );
        assert_eq!(
            env.get(velnor_actions_workflow_renderer::RELEASE_COMMIT_ENV),
            Some(&"f".repeat(40))
        );
        assert_eq!(job.steps[2].name, "Download plan");
        assert!(matches!(
            job.steps[3].kind,
            StepKind::NativeCheckExecution { .. }
        ));
        assert_eq!(
            job.steps.last().map(|step| step.condition.as_deref()),
            Some(Some("always()"))
        );
    }
}

#[test]
fn consumer_native_jobs_fail_closed_without_a_release_manifest() {
    let discovery = Discovery {
        native_checks: vec![native_check(
            "tap-tree",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        )],
        ..Discovery::default()
    };
    let error = build_check_jobs(
        WorkflowPolicy::ConsumerV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect_err("consumer native checks require artifact provenance");
    assert!(
        error
            .to_string()
            .contains("consumer_requires_release_install")
    );
}

#[test]
fn velnor_native_jobs_leave_acquisition_to_lock_or_preseed_attachment() {
    let discovery = Discovery {
        native_checks: vec![native_check(
            "tap-tree",
            runner("ubuntu-26.04", CheckPlatform::LinuxX64),
        )],
        ..Discovery::default()
    };
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("repository-owned native job");
    assert_eq!(jobs[0].1.steps[0].role, Some(StepRole::Checkout));
    assert!(
        jobs[0]
            .1
            .steps
            .iter()
            .all(|step| step.role != Some(StepRole::AcquireVelnor))
    );
}
