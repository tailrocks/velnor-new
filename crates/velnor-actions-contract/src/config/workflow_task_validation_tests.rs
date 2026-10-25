//! Shared workflow task identifier and capability validation.
use super::named;
use crate::config::{
    BuildTask, BuildTaskRunner, MiseTaskSource, NativeImageCachePolicy, NativeImagePlatform,
    NativeImageTask, VerificationRunner, VerificationTask, WorkflowTask,
};

#[test]
fn workflow_tasks_require_sorted_unique_safe_ids() {
    let make = |id: &str| {
        WorkflowTask::Verification(VerificationTask {
            id: id.to_owned(),
            mise_task: format!("check-{id}"),
            source: MiseTaskSource::default(),
            runner: VerificationRunner::LinuxX64,
            timeout_minutes: 5,
        })
    };
    let mut valid = named("CI");
    valid.tasks = vec![make("native-format"), make("native-lint")];
    assert!(valid.validate("config.toml").is_ok());

    valid.tasks.reverse();
    let error = valid.validate("config.toml").expect_err("unsorted fails");
    assert!(error.to_string().contains("tasks_must_be_sorted_by_id"));

    valid.tasks = vec![make("native-lint"), make("native-lint")];
    let error = valid.validate("config.toml").expect_err("duplicate fails");
    assert!(error.to_string().contains("duplicate_workflow_task_id"));

    valid.tasks = vec![make("required")];
    let error = valid
        .validate("config.toml")
        .expect_err("reserved ID fails");
    assert!(error.to_string().contains("bad_workflow_task_id"));
}

#[test]
fn workflow_task_variants_share_sorted_unique_ids_and_build_bound() {
    let make_build = |id: &str| {
        WorkflowTask::Build(BuildTask {
            id: id.to_owned(),
            mise_task: "desktop-ci".to_owned(),
            source: MiseTaskSource::default(),
            tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 5,
            cargo_build_jobs: 2,
            nextest_test_threads: 2,
        })
    };
    let mut valid = named("CI");
    valid.tasks = vec![make_build("native-desktop")];
    assert!(valid.validate("config.toml").is_ok());

    valid.tasks = vec![make_build("native-desktop"), make_build("native-ios")];
    let error = valid
        .validate("config.toml")
        .expect_err("multiple native build tasks fail");
    assert!(
        error
            .to_string()
            .contains("more_than_one_native_build_task")
    );

    valid.tasks = vec![make_build("required")];
    let error = valid
        .validate("config.toml")
        .expect_err("reserved build task ID fails");
    assert!(error.to_string().contains("bad_workflow_task_id"));

    valid.tasks = vec![
        make_build("native-desktop"),
        WorkflowTask::Verification(VerificationTask {
            id: "native-desktop".to_owned(),
            mise_task: "desktop-format-check".to_owned(),
            source: MiseTaskSource::default(),
            runner: VerificationRunner::MacosArm64,
            timeout_minutes: 5,
        }),
    ];
    let error = valid
        .validate("config.toml")
        .expect_err("duplicate IDs across variants fail");
    assert!(error.to_string().contains("duplicate_workflow_task_id"));
}

#[test]
fn native_image_tasks_share_ids_and_have_one_hosted_capability_slot() {
    let image = |id: &str| {
        WorkflowTask::NativeImage(NativeImageTask {
            id: id.to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
            timeout_minutes: 5,
            cache: NativeImageCachePolicy::TaskOwnedBuilder,
        })
    };
    let mut valid = named("CI");
    valid.tasks = vec![image("architect-arm64-image")];
    assert!(valid.validate("config.toml").is_ok());

    valid.tasks = vec![image("architect-arm64-image"), image("other-image")];
    let error = valid
        .validate("config.toml")
        .expect_err("multiple image tasks exceed the one-host resource envelope");
    assert!(
        error
            .to_string()
            .contains("more_than_one_native_image_task")
    );

    valid.tasks = vec![image("required")];
    let error = valid
        .validate("config.toml")
        .expect_err("image tasks use the workflow-wide ID namespace");
    assert!(error.to_string().contains("bad_workflow_task_id"));
}
