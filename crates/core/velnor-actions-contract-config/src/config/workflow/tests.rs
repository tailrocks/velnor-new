use super::{GeneratorValidation, WorkflowConfig, WorkflowPolicy};
use crate::config::{VerificationRunner, VerificationTask, VerificationTaskKind};
use velnor_actions_contract_release::targets::RUNNER_LABEL_CATALOG;

/// Workflow config carrying `name`, all else default.
fn named(name: &str) -> WorkflowConfig {
    WorkflowConfig {
        name: name.to_owned(),
        policy: WorkflowPolicy::ConsumerV1,
        default_branch: None,
        generator_validation: GeneratorValidation::Bootstrap,
        max_parallel_jobs: 2,
        runner_label: None,
        tasks: Vec::new(),
    }
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
        RUNNER_LABEL_CATALOG
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
    let make = |id: &str| VerificationTask {
        id: id.to_owned(),
        kind: VerificationTaskKind::Verification,
        mise_task: format!("check-{id}"),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 10,
    };
    let mut valid = named("CI");
    valid.tasks = vec![make("native-format"), make("native-lint")];
    assert!(valid.validate("config.toml").is_ok());

    valid.tasks.reverse();
    let error = valid.validate("config.toml").expect_err("unsorted fails");
    assert!(error.to_string().contains("tasks_must_be_sorted_by_id"));

    valid.tasks = vec![make("native-lint"), make("native-lint")];
    let error = valid.validate("config.toml").expect_err("duplicate fails");
    assert!(error.to_string().contains("duplicate_verification_task"));

    valid.tasks = vec![make("required")];
    let error = valid
        .validate("config.toml")
        .expect_err("reserved ID fails");
    assert!(error.to_string().contains("bad_verification_task_id"));
}
