use super::{
    GeneratorValidation, PullRequestCachePolicy, VerifyConfig, WorkflowConfig, WorkflowPolicy,
};
use crate::config::{
    BuildTask, BuildTaskRunner, NativeImageCachePolicy, NativeImagePlatform, NativeImageTask,
    VerificationRunner, VerificationTask, WorkflowTask,
};

/// Workflow config carrying `name`, all else default.
fn named(name: &str) -> WorkflowConfig {
    WorkflowConfig {
        name: name.to_owned(),
        policy: WorkflowPolicy::ConsumerV1,
        default_branch: None,
        generator_validation: GeneratorValidation::Bootstrap,
        max_parallel_jobs: 2,
        pull_request_cache_policy: PullRequestCachePolicy::default(),
        runner_label: None,
        tasks: Vec::new(),
        tofu_apply: None,
        verify: VerifyConfig::default(),
    }
}

#[test]
fn pull_request_cache_policy_is_strict_and_uses_kebab_case() {
    for (policy, wire) in [
        (PullRequestCachePolicy::ReadOnly, "\"read-only\""),
        (
            PullRequestCachePolicy::SameRepositoryScoped,
            "\"same-repository-scoped\"",
        ),
    ] {
        let serialized = serde_json::to_string(&policy).expect("serialize policy");
        assert_eq!(serialized, wire);
        assert_eq!(
            serde_json::from_str::<PullRequestCachePolicy>(wire).expect("deserialize policy"),
            policy
        );
    }
    assert_eq!(
        PullRequestCachePolicy::default(),
        PullRequestCachePolicy::ReadOnly
    );
    assert!(serde_json::from_str::<PullRequestCachePolicy>("\"same-repo\"").is_err());
}

#[test]
fn workflow_cache_policy_defaults_when_missing_and_serializes_opt_in() {
    let mut value = serde_json::json!({
        "name": "CI",
        "policy": "consumer-v1",
        "generator_validation": "bootstrap",
        "max_parallel_jobs": 2
    });
    let config: WorkflowConfig =
        serde_json::from_value(value.clone()).expect("deserialize default workflow");
    assert_eq!(
        config.pull_request_cache_policy,
        PullRequestCachePolicy::ReadOnly
    );

    value["pull_request_cache_policy"] =
        serde_json::Value::String("same-repository-scoped".to_owned());
    let config: WorkflowConfig =
        serde_json::from_value(value).expect("deserialize opted-in workflow");
    assert_eq!(
        serde_json::to_value(config)
            .expect("serialize workflow")
            .get("pull_request_cache_policy"),
        Some(&serde_json::Value::String(
            "same-repository-scoped".to_owned()
        ))
    );
}

#[test]
fn workflow_name_rejects_expressions_and_controls() {
    assert!(named("CI").validate("config.toml").is_ok());
    for name in ["${{ github.ref }}", "a\nb", "a\rb", "a\tb"] {
        let err = named(name)
            .validate("config.toml")
            .expect_err("bad name fails");
        assert!(err.to_string().contains("bad_name"), "{err}");
    }
}

#[test]
fn generic_workflow_runner_catalog_is_linux_only() {
    assert!(
        super::RUNNER_LABEL_CATALOG
            .iter()
            .all(|label| label.starts_with("ubuntu-"))
    );
    let mut config = named("CI");
    config.runner_label = Some("macos-15".to_owned());
    assert!(
        config
            .validate("config.toml")
            .expect_err("macOS checks use their separate typed runner config")
            .to_string()
            .contains("unsupported_label:macos-15")
    );
}

#[test]
fn workflow_tasks_require_sorted_unique_safe_ids() {
    let make = |id: &str| {
        WorkflowTask::Verification(VerificationTask {
            id: id.to_owned(),
            mise_task: format!("check-{id}"),
            source: crate::config::VerificationTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: VerificationRunner::LinuxX64,
            timeout_minutes: 10,
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
            tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 120,
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
            source: crate::config::VerificationTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: VerificationRunner::MacosArm64,
            timeout_minutes: 10,
        }),
    ];
    let error = valid
        .validate("config.toml")
        .expect_err("duplicate IDs across variants fail");
    assert!(error.to_string().contains("duplicate_workflow_task_id"));
}

#[test]
fn default_branch_rejects_yaml_and_git_injection() {
    for branch in [
        "",
        "feature/x y",
        "main\non: [push]",
        "main;git status",
        "${{ github.ref }}",
        "release/../main",
        "main.lock",
    ] {
        let mut config = named("CI");
        config.default_branch = Some(branch.to_owned());
        let err = config
            .validate("config.toml")
            .expect_err("malformed default branch must fail");
        assert!(err.to_string().contains("malformed_branch"), "{err}");
    }

    let mut config = named("CI");
    config.default_branch = Some("release/1.2".to_owned());
    assert_eq!(config.validate("config.toml"), Ok(()));
}

#[test]
fn native_image_tasks_share_ids_and_have_one_hosted_capability_slot() {
    let image = |id: &str| {
        WorkflowTask::NativeImage(NativeImageTask {
            id: id.to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
            timeout_minutes: 60,
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

#[test]
fn verify_jobs_accept_known_ids_in_any_order() {
    let mut config = named("CI");
    config.verify.jobs = vec![
        "native-validators".to_owned(),
        "alint".to_owned(),
        "strict-json".to_owned(),
    ];
    assert!(config.validate("config.toml").is_ok());
}

#[test]
fn verify_jobs_reject_unknown_and_duplicates() {
    let mut config = named("CI");
    config.verify.jobs = vec!["bogus".to_owned()];
    let err = config
        .validate("config.toml")
        .expect_err("unknown job fails");
    assert!(
        err.to_string().contains("unknown_verify_job:bogus"),
        "{err}"
    );
    for forbidden in ["cargo-deny", "cargo-machete", "actionlint", "plan"] {
        config.verify.jobs = vec![forbidden.to_owned()];
        let err = config
            .validate("config.toml")
            .expect_err("non-verify ID fails");
        assert!(
            err.to_string().contains("unknown_verify_job"),
            "{forbidden}: {err}"
        );
    }
    config.verify.jobs = vec!["alint".to_owned(), "alint".to_owned()];
    let err = config
        .validate("config.toml")
        .expect_err("duplicate job fails");
    assert!(
        err.to_string().contains("duplicate_verify_job:alint"),
        "{err}"
    );
}
