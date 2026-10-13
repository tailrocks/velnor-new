//! Opaque execution eligibility remains separate from source-change classification.
use super::*;
use velnor_actions_contract_config::MiseCheck;
use velnor_actions_contract_workflow::{ObligationDecision, PlanGenerator};
use velnor_actions_mise::ToolCatalog;
#[test]
fn unchanged_named_check_executes_without_fabricating_a_source_change() {
    let temp = tempfile::tempdir().expect("repository");
    std::fs::write(
        temp.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo checked'\n",
    )
    .expect("task");
    std::fs::write(temp.path().join("input.txt"), "unchanged").expect("input");
    let check: MiseCheck = serde_json::from_value(serde_json::json!({
        "id": "verify", "task": "verify", "directory": ".",
        "runner": { "label": "ubuntu-24.04", "platform": "linux_x64", "executor": "hosted" },
        "inputs": ["input.txt"], "tools": [], "system_tools": [], "timeout_minutes": 10
    }))
    .expect("check");
    let catalog = ToolCatalog::pinned();
    let item = velnor_actions_mise::discover_checks(temp.path(), &[check], &[])
        .expect("discovery")
        .remove(0);
    assert!(!group_changed(
        &item.proposal,
        &BTreeSet::new(),
        &BTreeSet::new()
    ));
    let generator = PlanGenerator {
        version: "0.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
    };
    let argv =
        velnor_actions_orchestrator_provisioning::vectors::task_argv(&item.proposal, &catalog)
            .expect("argv");
    let (obligation, entry) =
        velnor_actions_orchestrator_graph::internal_plan::named_checks::plan::derive(
            temp.path(),
            &item,
            "local",
            &generator,
            &catalog,
            &argv,
        )
        .expect("obligation");
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    assert_eq!(obligation.reason, "opaque_check_requires_execution");
    assert_eq!(entry.task_id, obligation.task_id);
    assert_eq!(entry.input_digest, obligation.input_digest);
    assert!(
        !entry.adapter_metadata["task_cache_enabled"]
            .as_bool()
            .expect("cache policy")
    );
}
