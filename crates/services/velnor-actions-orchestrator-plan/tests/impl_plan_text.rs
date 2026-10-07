//! Deterministic plan text over a consumer preparation.

use std::collections::BTreeMap;

use velnor_actions_orchestrator_generation::finalized::finalized_jobs;
use velnor_actions_orchestrator_generation::prepare::{GenerationPreparation, prepare};
use velnor_actions_orchestrator_plan::plan::{plan_text, plan_text_checked};

/// Consumer fixture root carrying config plus the release install receipt.
fn consumer_prep() -> (tempfile::TempDir, GenerationPreparation) {
    let root = tempfile::TempDir::new().expect("root");
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'consumer-v1'\ndefault_branch = 'main'\n",
    )
    .expect("config");
    std::fs::write(
        root.path().join(".velnor/release-manifest.json"),
        include_str!("../../../../fixtures/consumer-release-manifest.json"),
    )
    .expect("manifest");
    let prep = prepare(root.path()).expect("consumer preparation");
    (root, prep)
}

#[test]
fn checked_plan_renders_header_branch_and_runner() {
    let (_root, prep) = consumer_prep();
    let text = plan_text_checked(&prep).expect("plan text");
    assert!(text.contains("Velnor Actions plan 0.1.1"), "{text}");
    assert!(text.contains("Push branch: main"), "{text}");
    assert!(
        text.contains("Runner: ubuntu-26.04 (latest pinned default)"),
        "{text}"
    );
}

#[test]
fn checked_plan_lists_finalized_jobs() {
    let (_root, prep) = consumer_prep();
    let text = plan_text_checked(&prep).expect("plan text");
    assert!(text.contains("    - plan ("), "{text}");
    assert!(text.contains("    - required ("), "{text}");
}

#[test]
fn plan_text_is_deterministic() {
    let (_root, prep) = consumer_prep();
    let jobs = finalized_jobs(&prep).expect("finalized jobs");
    assert_eq!(plan_text(&prep, &jobs), plan_text(&prep, &jobs));
}

#[test]
fn plan_text_without_jobs_keeps_header() {
    let (_root, prep) = consumer_prep();
    let text = plan_text(&prep, &BTreeMap::new());
    assert!(text.contains("Velnor Actions plan 0.1.1"), "{text}");
    assert!(text.contains("Workflow to generate"), "{text}");
}
