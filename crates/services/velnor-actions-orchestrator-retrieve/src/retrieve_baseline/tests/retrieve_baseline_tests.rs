use super::*;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    BaselineProof, ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix,
    PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_merge_ports::{BaselineManifest, BaselineTaskEntry};

#[test]
fn baseline_skips_unparsable_plan_without_spawning() {
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

#[test]
fn baseline_skips_when_staged_file_wins() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let staged = tmp.path().join(super::super::BASELINE_FILENAME);
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
fn baseline_skips_invalid_base_before_branch_lookup() {
    let plan = covered_plan_value(Some("zz"));
    assert!(!attempt(&plan, "o/r"));
}

#[test]
fn baseline_skips_bad_repo_before_branch_lookup() {
    let plan = covered_plan_value(Some(&"1".repeat(40)));
    assert!(!attempt(&plan, "not a slug!!"));
}

#[test]
fn default_branch_args_pins_repo_and_rejects_bad_slug() {
    let args = default_branch_args("o/r");
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(text, ["api", "repos/o/r", "--jq", ".default_branch"]);
    assert!(default_branch_args("not a slug!!").is_empty());
    assert!(default_branch_args("").is_empty());
}

#[test]
fn parse_default_branch_accepts_only_strict_names() {
    assert_eq!(parse_default_branch("main\n").as_deref(), Some("main"));
    assert_eq!(parse_default_branch("\"main\"\n").as_deref(), Some("main"));
    assert_eq!(
        parse_default_branch("feature/x").as_deref(),
        Some("feature/x")
    );
    for bad in [
        "",
        "   \n",
        "HEAD",
        "a b",
        "a\tb",
        "../x",
        "x/../y",
        "a*b",
        "a$b",
        "a;b",
        "x://y",
        "a\"b",
        "\"unterminated",
    ] {
        assert_eq!(parse_default_branch(bad), None, "rejects {bad:?}");
    }
}

#[test]
fn stage_manifest_writes_canonically_and_never_overwrites() {
    let manifest = manifest_for(&"1".repeat(40));
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(stage_manifest(tmp.path(), &manifest));
    let staged = tmp.path().join(super::super::BASELINE_FILENAME);
    let bytes = std::fs::read(&staged).expect("read");
    assert_eq!(bytes, canonical_json_bytes(&manifest).expect("canonical"));
    assert!(!stage_manifest(tmp.path(), &manifest));
    assert_eq!(std::fs::read(&staged).expect("reread"), bytes);
}

#[cfg(unix)]
#[test]
fn baseline_skips_when_staged_symlink_planted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target.json");
    std::fs::write(&target, "{}").expect("target");
    std::os::unix::fs::symlink(&target, tmp.path().join(super::super::BASELINE_FILENAME))
        .expect("link");
    let plan = covered_plan_value(Some("zz"));
    assert!(!retrieve_baseline_to(
        &ToolCatalog::pinned(),
        tmp.path(),
        &plan,
        "o/r"
    ));
    assert!(!stage_manifest(tmp.path(), &manifest_for(&"1".repeat(40))));
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
    // The proof constructor rejects zero, so the zero-id mutation case
    // proves rejection via the manifest conjuncts, never a forged proof.
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

        artifact_tasks: Vec::new(),
    }
}

/// Covered plan value over `base` (strict round-trip like `plan.json`).
fn covered_plan_value(base: Option<&str>) -> serde_json::Value {
    let manifest = manifest_for(&"1".repeat(40));
    let plan = plan_for(&manifest, base);
    serde_json::to_value(&plan).expect("plan value")
}
