//! A matching baseline never authorizes skipping an opaque check.
use super::*;

use velnor_actions_contract::{CheckExecutor, CheckPlatform, CheckRunner, MiseCheck};
#[test]
fn exact_baseline_match_still_executes_named_check() {
    let temp = tempfile::tempdir().expect("repository");
    std::fs::write(
        temp.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo verified'\n",
    )
    .expect("task");
    std::fs::write(temp.path().join("input.txt"), "bound source").expect("input");
    let check = MiseCheck {
        id: "verify".to_owned(),
        task: "verify".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["input.txt".to_owned()],
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let item = velnor_actions_mise::discover_checks(temp.path(), &[check], &[])
        .expect("discovery")
        .remove(0);
    let id = item.proposal.task_id.clone();
    let mut plan = plan_with(&[&id]);
    let (obligation, entry) = crate::internal_plan::named_checks::plan::derive(
        temp.path(),
        &item,
        &plan.run_key,
        &plan.generator,
        &catalog,
    )
    .expect("plan");
    plan.obligations = vec![obligation];
    plan.matrix.include = vec![entry];
    let mut manifest = manifest_with(&[(&id, &plan.obligations[0].closure_digest)]);
    manifest.tasks[0].task_digest = plan.obligations[0].task_digest.clone();
    manifest.tasks[0].input_digest = plan.obligations[0].input_digest.clone();
    let mut discovery = discovery_with(&[]);
    discovery.proposals = vec![item.proposal.clone()];
    discovery.mise_checks = vec![item];
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &discovery,
        Some(&BTreeSet::new()),
        &inputs(temp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert_eq!(plan.obligations[0].decision, ObligationDecision::Execute);
    assert!(plan.obligations[0].baseline_proof.is_none());
    assert_eq!(plan.matrix.include.len(), 1);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.ends_with(":undeclared_inputs"))
    );
}
