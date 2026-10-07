//! Validated generation input plus the emission selectors over it.

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_generation::finalized::finalized_jobs;
use velnor_actions_orchestrator_generation::freshness_emit::freshness_enabled;
use velnor_actions_orchestrator_generation::prepare::{GenerationPreparation, prepare};
use velnor_actions_orchestrator_generation::release_emit::enabled_release;

/// Consumer fixture root carrying config plus the release install receipt.
fn consumer_root(branch: Option<&str>) -> tempfile::TempDir {
    let root = tempfile::TempDir::new().expect("root");
    let mut config = "schema = 1\n[workflow]\npolicy = 'consumer-v1'\n".to_owned();
    if let Some(branch) = branch {
        config.push_str(&format!("default_branch = '{branch}'\n"));
    }
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(root.path().join(".velnor/config.toml"), config).expect("config");
    std::fs::write(
        root.path().join(".velnor/release-manifest.json"),
        include_str!("../../../../fixtures/consumer-release-manifest.json"),
    )
    .expect("manifest");
    root
}

fn consumer_prep() -> (tempfile::TempDir, GenerationPreparation) {
    let root = consumer_root(Some("main"));
    let prep = prepare(root.path()).expect("consumer preparation");
    (root, prep)
}

#[test]
fn prepare_accepts_consumer_repo_with_default_branch() {
    let (_root, prep) = consumer_prep();
    assert_eq!(prep.default_branch, "main");
    assert!(prep.discovery.proposals.is_empty());
    assert!(prep.lock_audit_blocking.is_empty());
}

#[test]
fn prepare_rejects_missing_default_branch_without_git() {
    let root = consumer_root(None);
    let err = prepare(root.path()).expect_err("no branch and no git");
    assert!(
        matches!(err, OrchestratorError::DefaultBranch { .. }),
        "wrong error: {err}"
    );
}

#[test]
fn prepare_rejects_non_directory() {
    let root = tempfile::TempDir::new().expect("root");
    let missing = root.path().join("no-such-dir");
    assert!(prepare(&missing).is_err());
}

#[test]
fn prepare_reports_default_runner_label() {
    let (_root, prep) = consumer_prep();
    assert_eq!(prep.runner_label, "ubuntu-26.04");
}

#[test]
fn freshness_disabled_for_consumer() {
    let (_root, prep) = consumer_prep();
    assert!(!freshness_enabled(&prep));
}

#[test]
fn release_disabled_without_release_config() {
    let (_root, prep) = consumer_prep();
    assert!(enabled_release(&prep).expect("release query").is_none());
}

#[test]
fn finalized_jobs_lists_plan_and_required() {
    let (_root, prep) = consumer_prep();
    let jobs = finalized_jobs(&prep).expect("finalized jobs");
    assert!(jobs.contains_key("plan"), "plan job: {}", jobs.len());
    assert!(jobs.contains_key("required"), "required job");
}
