//! Lookup derivation tests: exact names without spawning.
//!
//! Declared via `#[path]` from `cover_baseline.rs` under `cfg(test)`;
//! the entry-test file is at the size gate.

use super::*;

use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ObligationDecision, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust,
};

/// Plan with `base`, one obligation, and a verifiable generator.
fn lookup_plan(base: Option<&str>) -> Plan {
    let digest = digest_b3(b"digest");
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: base.map(str::to_owned),
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "1".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: digest.clone(),
            input_digest: digest.clone(),
            closure_digest: digest,
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: vec!["stack/rust/root/clippy/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

/// Lookup inputs over a bare temp root.
fn lookup_inputs<'a>(
    root: &'a Path,
    catalog: &'a velnor_actions_mise::ToolCatalog,
) -> BaselineInputs<'a> {
    BaselineInputs {
        branch: "testmain",
        root,
        workflow: ".github/workflows/ci.yml",
        catalog,
        repository: Some("o/r"),
    }
}

#[test]
fn lookup_names_exact_baseline_artifact() {
    let base = "a".repeat(40);
    let plan = lookup_plan(Some(&base));
    let compat = crate::cover_compat::baseline_compat_for_plan(&plan).expect("compat");
    let expected =
        velnor_actions_contract::artifact_id_for_baseline(&base, &compat).expect("artifact name");
    assert_eq!(lookup_artifact_name(&plan, &base).expect("name"), expected);
}

#[test]
fn lookup_misses_before_spawn_without_exact_name() {
    let plan = lookup_plan(Some(&"a".repeat(40)));
    for bad in ["abc123", &"A".repeat(40), "", "a".repeat(39).as_str()] {
        assert_eq!(
            lookup_artifact_name(&plan, bad).expect_err("must miss"),
            "baseline_no_exact_artifact".to_owned(),
            "malformed base names no artifact: {bad:?}"
        );
    }
}

#[test]
fn lookup_manifest_misses_without_base_or_obligations() {
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut baseless = lookup_plan(None);
    assert!(lookup_manifest(&mut baseless, lookup_inputs(tmp.path(), &catalog)).is_none());
    assert_eq!(baseless.baseline.reason(), Some("baseline_no_base"));
    let mut empty = lookup_plan(Some(&"a".repeat(40)));
    empty.obligations.clear();
    assert!(lookup_manifest(&mut empty, lookup_inputs(tmp.path(), &catalog)).is_none());
    assert_eq!(empty.baseline.reason(), Some("baseline_no_obligations"));
}
