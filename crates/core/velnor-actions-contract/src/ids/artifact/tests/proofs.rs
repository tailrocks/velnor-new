use super::{proof_commit, proof_digest};
use velnor_actions_contract_workflow::workflow::baseline::{
    BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline,
};

#[test]
fn proof_constructor_validates_every_input() {
    let digest = proof_digest();
    let commit = proof_commit();
    let proof =
        BaselineProof::new(&commit, 7, 9, "velnor-plan-local", &digest).expect("valid proof");
    assert_eq!(proof.run_id(), 7);
    assert_eq!(proof.artifact_id(), 9);
    assert_eq!(proof.source_commit(), commit);
    assert_eq!(proof.artifact_name(), "velnor-plan-local");
    assert_eq!(proof.manifest_digest(), digest);
    proof.validate().expect("revalidate");
    assert!(BaselineProof::new("short", 7, 9, "velnor-plan-local", &digest).is_err());
    assert!(BaselineProof::new(&commit, 0, 9, "velnor-plan-local", &digest).is_err());
    assert!(BaselineProof::new(&commit, 7, 0, "velnor-plan-local", &digest).is_err());
    assert!(BaselineProof::new(&commit, 7, 9, "velnor-nope-x", &digest).is_err());
    assert!(BaselineProof::new(&commit, 7, 9, "velnor-plan-local", "b3-nope").is_err());
    let json = serde_json::to_string(&proof).expect("serialize");
    assert!(serde_json::from_str::<BaselineProof>(&json).is_ok());
    let forged = json.replace(&digest, "b3-nope");
    assert!(serde_json::from_str::<BaselineProof>(&forged).is_err());
}

#[test]
fn baseline_states_are_exhaustive() {
    let digest = proof_digest();
    let commit = proof_commit();
    let used = PlanBaseline::used(&commit, 7, 9, "velnor-plan-local", &digest).expect("used");
    assert_eq!(used.status(), BaselineStatus::Used);
    assert!(PlanBaseline::used("short", 7, 9, "velnor-plan-local", &digest).is_err());
    let mut stale = used;
    stale.mark_unavailable("baseline_expired").expect("mark");
    assert_eq!(stale.status(), BaselineStatus::Unavailable);
    assert_eq!(stale.reason(), Some("baseline_expired"));
    assert!(stale.mark_unavailable("").is_err());
    let wire = serde_json::to_string(&stale).expect("serialize");
    assert!(!wire.contains("base_commit"));
    let with_stale = r#"{"status":"unavailable","run_id":7}"#;
    assert!(serde_json::from_str::<PlanBaseline>(with_stale).is_err());
    assert!(serde_json::from_str::<PlanBaseline>(r#"{"status":"used"}"#).is_err());
    let fresh = PlanBaseline::used(&commit, 7, 9, "velnor-plan-local", &digest).expect("used");
    let mut wire_used: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&fresh).expect("ser")).expect("de");
    wire_used["reason"] = serde_json::json!("stale");
    assert!(serde_json::from_value::<PlanBaseline>(wire_used).is_err());
}

#[test]
fn task_proof_needs_valid_ids_and_digests() {
    let task = "stack/rust/root/clippy/default";
    let good = proof_digest();
    let build = |task: &str, td: &str, run: u64| {
        ManifestTaskProof::new(task, td, &good, &good, &good, &good, &good, "default", run)
    };
    let proof = build(task, &good, 7).expect("valid task proof");
    assert_eq!(proof.task_id(), task);
    assert_eq!(proof.proof_run_id(), 7);
    proof.validate().expect("revalidate");
    assert!(build("bogus", &good, 7).is_err());
    assert!(build(task, "b3-nope", 7).is_err());
    assert!(build(task, &good, 0).is_err());
    let json = serde_json::to_string(&proof).expect("serialize");
    assert!(serde_json::from_str::<ManifestTaskProof>(&json).is_ok());
    let forged = json.replace(&good, "b3-nope");
    assert!(serde_json::from_str::<ManifestTaskProof>(&forged).is_err());
}
