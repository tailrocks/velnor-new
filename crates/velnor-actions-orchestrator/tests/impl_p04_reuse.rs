//! P04 reuse matrix: sharded archive identities bind content and execute.
//!
//! Unregistered: the parent wires this module into `velnor_orchestrator.rs`
//! (P09 pattern). Cases run through the public plan API only.

use velnor_actions_contract::{ObligationDecision, Plan};

use super::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, write_nextest_task,
};
use super::impl_orch_core::plan_value;

/// Typed plan from a plan-response value.
fn typed(value: &serde_json::Value) -> Result<Plan, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(value["plan"].clone())?)
}

#[test]
fn sharded_plan_binds_archive_content_and_executes() -> TestResult {
    let config = format!(
        "{}\n[test_sharding]\ndefault_shards = 2\n",
        config_with_branch()
    );
    let dir = make_repo(&config)?;
    let root = dir.path();
    write_nextest_task(root)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan = typed(&plan_value(root, "push", Some(&head), &head)?)?;
    let mut shards = 0u32;
    for ob in &plan.obligations {
        if !ob.task_id.contains("/shard-") {
            continue;
        }
        shards += 1;
        // The archive identity binds the content closure, so the gate
        // clears on content instead of refusing as source-unbound; the
        // unqualified task cache still executes with its own reason.
        assert_eq!(ob.decision, ObligationDecision::Execute, "{ob:?}");
        assert_ne!(ob.reason, "archive_source_unbound", "{ob:?}");
        assert_eq!(ob.reason, "forced_uncached", "{ob:?}");
    }
    assert!(shards > 0, "{:?}", plan.task_ids);
    Ok(())
}
