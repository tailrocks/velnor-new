use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    BaselineProof, ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix,
    PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_merge_ports::{BaselineManifest, BaselineTaskEntry};
use velnor_actions_orchestrator_retrieve::retrieve_baseline::retrieve_baseline_to;

/// Fetch attempt over one plan value in a fresh run directory.
fn attempt(plan: &serde_json::Value, repo: &str) -> bool {
    let tmp = tempfile::tempdir().expect("tempdir");
    retrieve_baseline_to(&ToolCatalog::pinned(), tmp.path(), plan, repo)
}

#[test]
fn baseline_skips_unparsable_plan() {
    let plan = serde_json::json!({"schema": 1, "nope": true});
    assert!(!attempt(&plan, "o/r"));
}

#[test]
fn baseline_skips_plan_without_base() {
    assert!(!attempt(&covered_plan_value(None), "o/r"));
}

#[test]
fn baseline_skips_when_nothing_covered() {
    let manifest = manifest_for(&"1".repeat(40));
    let mut plan = plan_for(&manifest, Some(&"1".repeat(40)));
    for obligation in &mut plan.obligations {
        obligation.decision = ObligationDecision::Execute;
        obligation.baseline_proof = None;
    }
    let value = serde_json::to_value(&plan).expect("plan value");
    assert!(!attempt(&value, "o/r"));
}

/// Staged evidence wins by on-disk name: the literal pins the
/// value-matched filename, so any drift fails loudly here.
#[test]
fn baseline_skips_when_staged_wins() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let staged = tmp.path().join("baseline.json");
    std::fs::write(&staged, r#"{"staged":true}"#).expect("staged");
    let plan = covered_plan_value(Some("zz"));
    assert!(!retrieve_baseline_to(
        &ToolCatalog::pinned(),
        tmp.path(),
        &plan,
        "o/r"
    ));
    assert_eq!(
        std::fs::read_to_string(&staged).expect("reread"),
        r#"{"staged":true}"#
    );
}

#[test]
fn baseline_skips_bad_repo_before_lookup() {
    let plan = covered_plan_value(Some(&"1".repeat(40)));
    assert!(!attempt(&plan, "not a slug!!"));
}

#[test]
fn baseline_skips_invalid_base_before_lookup() {
    let plan = covered_plan_value(Some("zz"));
    assert!(!attempt(&plan, "o/r"));
}

/// Task and input digests shared by obligation and entry.
fn digests() -> (String, String, String) {
    (
        digest_b3(b"task"),
        digest_b3(b"inputs"),
        digest_b3(b"closure"),
    )
}

/// Trusted manifest with one entry over `commit`.
fn manifest_for(commit: &str) -> BaselineManifest {
    let (task, inputs, closure) = digests();
    let compat = digest_b3(b"compat");
    let name = format!("velnor-baseline-{commit}-{compat}");
    BaselineManifest {
        schema: 2,
        repository_id: digest_b3(b"repo"),
        source_commit: commit.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: compat.clone(),
        artifact_id:
            velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(
                &name,
            ),
        artifact_name: name,
        tasks: vec![BaselineTaskEntry {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            task_digest: task,
            input_digest: inputs,
            closure_digest: closure,
            proof_run_id: 7,
            observed_run_id: 7,
            external_data: None,
            proof: None,
        }],
        expires_at_unix: None,
    }
}

/// Plan with one covered obligation bound to `manifest`.
fn plan_for(manifest: &BaselineManifest, base: Option<&str>) -> Plan {
    let (task, inputs, closure) = digests();
    let digest = digest_b3(&canonical_json_bytes(manifest).expect("canonical"));
    let proof = BaselineProof::new(
        &manifest.source_commit,
        7,
        manifest.artifact_id.max(1),
        &manifest.artifact_name,
        &digest,
    )
    .expect("proof");
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
            decision: ObligationDecision::CoveredByTrustedBaseline,
            reason: "covered_by_trusted_baseline".to_owned(),
            task_digest: task,
            input_digest: inputs,
            closure_digest: closure,
            baseline_proof: Some(proof),
        }],
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: vec!["stack/rust/root/clippy/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

/// Covered plan value over `base` (strict round-trip like `plan.json`).
fn covered_plan_value(base: Option<&str>) -> serde_json::Value {
    let manifest = manifest_for(&"1".repeat(40));
    let plan = plan_for(&manifest, base);
    serde_json::to_value(&plan).expect("plan value")
}
