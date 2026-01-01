//! Named-check lane derivation and generator identity.

use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract_config::{CheckExecutor, CheckPlatform, CheckRunner, MiseCheck};
use velnor_actions_contract_workflow::{ObligationDecision, PlanGenerator};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};
use velnor_actions_orchestrator_graph::internal_plan::current_exe_sha256;
use velnor_actions_orchestrator_graph::internal_plan::named_checks::plan::{
    derive, derive_lanes_until,
};

/// Fixture repository with one native check.
fn fixture() -> (TempDir, MiseCheck) {
    let dir = TempDir::new().expect("temporary repository");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo check'\n",
    )
    .expect("native task");
    std::fs::write(dir.path().join("input.txt"), "first").expect("input");
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
    (dir, check)
}

fn discovered_check(root: &Path, check: &MiseCheck) -> DiscoveredCheck {
    velnor_actions_mise::discover_checks(root, std::slice::from_ref(check), &[])
        .expect("static checks")
        .remove(0)
}

fn generator() -> PlanGenerator {
    PlanGenerator {
        version: "0.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn argv_for(item: &DiscoveredCheck, catalog: &ToolCatalog) -> Vec<String> {
    velnor_actions_orchestrator_provisioning::vectors::task_argv(&item.proposal, catalog)
        .expect("argv")
}

#[test]
fn derive_binds_single_lane_obligation() {
    let (dir, check) = fixture();
    let item = discovered_check(dir.path(), &check);
    let catalog = ToolCatalog::pinned();
    let argv = argv_for(&item, &catalog);
    let (obligation, entry) =
        derive(dir.path(), &item, "local", &generator(), &catalog, &argv).expect("plan check");
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    assert_eq!(entry.job_id, "check-verify");
    assert_eq!(entry.task_id, obligation.task_id);
    assert_eq!(entry.input_digest, obligation.input_digest);
}

#[test]
fn derive_without_lanes_fails_closed() {
    let (dir, check) = fixture();
    let item = discovered_check(dir.path(), &check);
    let catalog = ToolCatalog::pinned();
    let argv = argv_for(&item, &catalog);
    derive_lanes_until(
        dir.path(),
        &item,
        "local",
        &generator(),
        &catalog,
        &[],
        None,
        &argv,
    )
    .expect_err("lanes required");
}

#[test]
fn current_exe_sha_is_release_comparable() {
    let sha = current_exe_sha256().expect("exe readable");
    assert_eq!(sha.len(), 64);
    assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn input_changes_rebind_identity() {
    let (dir, check) = fixture();
    let catalog = ToolCatalog::pinned();
    let derive_once = |dir: &TempDir| {
        let item = discovered_check(dir.path(), &check);
        let argv = argv_for(&item, &catalog);
        derive(dir.path(), &item, "local", &generator(), &catalog, &argv)
            .expect("plan")
            .0
    };
    let before = derive_once(&dir);
    std::fs::write(dir.path().join("input.txt"), "second").expect("edit");
    let after = derive_once(&dir);
    assert_ne!(before.input_digest, after.input_digest);
    assert_ne!(before.closure_digest, after.closure_digest);
}
