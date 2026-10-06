//! Carry publication preserves origin and requires current successful proof.

use super::*;
use velnor_actions_contract::{
    BaselineProof, FinalCounts, FinalReport, FinalStatus, JobConclusion, RequiredJobResult,
    final_report_id_for_run,
};

/// One obligation with an explicit decision.
pub(super) fn obligation_for(
    task_id: &str,
    task_digest: String,
    seed: u8,
    decision: ObligationDecision,
) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        job_id: "rust-demo".to_owned(),
        decision,
        reason: "test".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        execution_identity: velnor_actions_contract::TaskExecutionIdentity::new(
            &digest(seed + 30),
            &digest(seed + 40),
            &digest(seed + 50),
            &digest(seed + 60),
            "default",
        )
        .expect("execution identity"),
        baseline_proof: None,
    }
}

/// Exact successful current report staged beside the plan fixture.
pub(super) fn stage_report(dir: &Path, plan: &Plan) {
    let executed = plan
        .obligations
        .iter()
        .filter(|ob| ob.decision == ObligationDecision::Execute)
        .count();
    let mut expected: Vec<_> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.report_id.clone())
        .collect();
    expected.sort();
    let mut artifacts: Vec<_> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.artifact_id.clone())
        .collect();
    artifacts.sort();
    artifacts.dedup();
    let report = FinalReport {
        schema: 1,
        report_id: final_report_id_for_run(&plan.run_key).expect("report id"),
        run_key: plan.run_key.clone(),
        plan_id: plan.plan_id.clone(),
        expected_report_ids: expected,
        downloaded_artifact_ids: artifacts,
        required_job_results: vec![
            RequiredJobResult {
                job_id: "plan".to_owned(),
                conclusion: JobConclusion::Success,
            },
            RequiredJobResult {
                job_id: "rust-demo".to_owned(),
                conclusion: JobConclusion::Success,
            },
        ],
        status: FinalStatus::Passed,
        counts: FinalCounts {
            selected: plan.task_ids.len().try_into().expect("count"),
            executed: executed.try_into().expect("count"),
            covered: (plan.obligations.len() - executed)
                .try_into()
                .expect("count"),
            reused: 0,
            empty_partition: 0,
            failed: 0,
            cancelled: 0,
            blocked: 0,
            not_run: 0,
        },
        miss_reasons: Vec::new(),
    };
    report.validate().expect("report validates");
    fs::write(
        dir.join("final-report.json"),
        serde_json::to_string(&report).expect("report"),
    )
    .expect("stage report");
}

/// Parent qualified by remote provenance; child uses one or all old tasks.
fn carrying_fixture(all: bool) -> (PublishRequest, Plan, BaselineManifest) {
    let base = "b".repeat(40);
    let origin_plan = fixture_plan(&base, "r7-a1");
    let origin_request = publish_request(&request_json(&base)).expect("request");
    let mut parent = publish_manifest(&origin_request, &origin_plan, 7, 1, None).expect("parent");
    parent.expires_at_unix = Some(u64::MAX);
    parent.tasks[1].external_data = Some(crate::external_data::ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: digest(8),
        age_secs: 60,
    });
    let request = publish_request(&request_json(&"a".repeat(40))).expect("request");
    let mut child = fixture_plan(&request.head, "r8-a1");
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("parent bytes"));
    child.baseline = PlanBaseline::used(
        &base,
        parent.run_id,
        parent.artifact_id,
        &parent.artifact_name,
        &digest,
    )
    .expect("planned parent");
    for obligation in &mut child.obligations {
        if all || obligation.task_id.contains("/test/") {
            obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
            obligation.baseline_proof = Some(
                BaselineProof::new(&base, 7, parent.artifact_id, &parent.artifact_name, &digest)
                    .expect("proof"),
            );
        }
    }
    child.matrix.include.retain(|entry| {
        child
            .obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    child.validate().expect("child");
    (request, child, parent)
}

#[test]
fn publication_carries_original_execution_through_mixed_and_empty_matrices() {
    for all in [false, true] {
        let (request, plan, parent) = carrying_fixture(all);
        evidence::qualify_parent(&request, &plan, &parent).expect("qualified");
        let manifest = publish_manifest(&request, &plan, 8, 1, Some(&parent)).expect("carry");
        self_check(&request, &manifest).expect("consumable");
        assert_eq!(manifest.tasks.len(), plan.obligations.len());
        assert_eq!(manifest.expires_at_unix, parent.expires_at_unix);
        for task in &manifest.tasks {
            assert_eq!(task.observed_run_id, 8);
            let covered = all || task.task_id.contains("/test/");
            assert_eq!(task.proof_run_id, if covered { 7 } else { 8 });
            assert_eq!(task.carried_from.is_some(), covered);
            if covered {
                let origin = parent
                    .tasks
                    .iter()
                    .find(|origin| origin.task_id == task.task_id)
                    .expect("origin");
                assert_eq!(task.external_data, origin.external_data);
            }
        }
        assert!(manifest.parent.is_some());
        assert_eq!(plan.matrix.include.is_empty(), all);
    }
}

#[test]
fn publication_rejects_invalid_parent_identity_and_provenance() {
    let (request, plan, parent) = carrying_fixture(true);
    for mutation in [
        "source",
        "repository",
        "status",
        "input",
        "closure",
        "proof",
        "expiry",
    ] {
        let mut bad = parent.clone();
        match mutation {
            "source" => bad.source_commit = "c".repeat(40),
            "repository" => bad.repository_id = digest(9),
            "status" => bad.final_status = "failed".to_owned(),
            "input" => bad.tasks[0].input_digest = digest(9),
            "closure" => bad.tasks[0].closure_digest = digest(9),
            "proof" => bad.tasks[0].proof_run_id = 99,
            "expiry" => bad.expires_at_unix = Some(1),
            _ => unreachable!("closed mutations"),
        }
        assert!(
            evidence::qualify_parent(&request, &plan, &bad).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn consecutive_covered_runs_preserve_original_execution() {
    let (request, plan, parent) = carrying_fixture(true);
    evidence::qualify_parent(&request, &plan, &parent).expect("parent");
    let carried = publish_manifest(&request, &plan, 8, 1, Some(&parent)).expect("second run");
    for next_head in ["c".repeat(40), carried.source_commit.clone()] {
        let next_request = publish_request(&request_json(&next_head)).expect("request");
        let mut next = fixture_plan(&next_request.head, "r9-a1");
        next.base = Some(carried.source_commit.clone());
        next.matrix.include.clear();
        let digest = digest_b3(&canonical_json_bytes(&carried).expect("carried bytes"));
        next.baseline = PlanBaseline::used(
            &carried.source_commit,
            carried.run_id,
            carried.artifact_id,
            &carried.artifact_name,
            &digest,
        )
        .expect("planned parent");
        for obligation in &mut next.obligations {
            obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
            obligation.baseline_proof = Some(
                BaselineProof::new(
                    &carried.source_commit,
                    7,
                    carried.artifact_id,
                    &carried.artifact_name,
                    &digest,
                )
                .expect("proof"),
            );
        }
        evidence::qualify_parent(&next_request, &next, &carried).expect("second parent");
        let third =
            publish_manifest(&next_request, &next, 9, 1, Some(&carried)).expect("third run");
        self_check(&next_request, &third).expect("third consumable");
        assert_eq!(third.tasks.len(), 2);
        assert!(
            third
                .tasks
                .iter()
                .all(|task| task.proof_run_id == 7 && task.observed_run_id == 9)
        );
        assert!(third.parent.as_ref().expect("parent").parent.is_some());
    }
}

#[test]
fn publication_requires_current_complete_passed_report() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    for mutation in [
        "absent",
        "failed",
        "cancelled",
        "wrong_run",
        "missing_report",
        "counts",
        "missing_job",
        "cancelled_job",
    ] {
        let temp = staged_run(&plan, "r7-a1");
        let path = temp.path().join("velnor/r7-a1/final-report.json");
        if mutation == "absent" {
            fs::remove_file(&path).expect("remove");
        } else {
            let mut report: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
            match mutation {
                "failed" | "cancelled" => report["status"] = serde_json::json!(mutation),
                "wrong_run" => report["run_key"] = serde_json::json!("r9-a1"),
                "missing_report" => report["expected_report_ids"] = serde_json::json!([]),
                "counts" => report["counts"]["executed"] = serde_json::json!(0),
                "missing_job" => report["required_job_results"] = serde_json::json!([]),
                "cancelled_job" => {
                    report["required_job_results"][1]["conclusion"] = serde_json::json!("cancelled")
                }
                _ => unreachable!("closed mutations"),
            }
            fs::write(path, report.to_string()).expect("write");
        }
        assert!(
            baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn staged_parent_never_replaces_remote_authenticated_acquisition() {
    let (_, _, parent) = carrying_fixture(true);
    let temp = tempfile::tempdir().expect("temp");
    fs::write(
        temp.path().join(BASELINE_FILENAME),
        canonical_json_bytes(&parent).expect("bytes"),
    )
    .expect("plant");
    let refused = evidence::acquire_parent(temp.path(), |fresh| {
        assert_ne!(fresh, temp.path());
        assert!(!fresh.join(BASELINE_FILENAME).exists());
        false
    });
    assert!(
        refused
            .expect_err("missing remote")
            .to_string()
            .contains("parent_unavailable")
    );
    let acquired = evidence::acquire_parent(temp.path(), |fresh| {
        fs::write(
            fresh.join(BASELINE_FILENAME),
            canonical_json_bytes(&parent).expect("bytes"),
        )
        .expect("remote stage");
        true
    })
    .expect("remote");
    assert_eq!(acquired.run_id, parent.run_id);
}
