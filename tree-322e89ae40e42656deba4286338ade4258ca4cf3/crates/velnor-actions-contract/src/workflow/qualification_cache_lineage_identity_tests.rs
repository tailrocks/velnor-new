use super::*;

use crate::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, PlanBaseline, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, PlannedPlatform, QualificationDispatch,
    QualificationPhase, QualificationRunRef, WorkflowEvent,
};
use crate::{
    RunnerSelection, Trust, digest_b3,
    ids::{plan_id_for_run, run_key_for_ci},
};

#[test]
fn cache_identity_survives_checkout_relocation_and_useful_source_edit() {
    let before = plan_for_checkout(
        "/home/runner/work/velnor/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let after = plan_for_checkout(
        "/tmp/agent/_work/velnor/velnor",
        45,
        "b".repeat(40),
        "input-after-source-edit",
        "closure-after-source-edit",
    );
    let before_matrix = &before.matrix.include[0];
    let after_matrix = &after.matrix.include[0];

    assert_eq!(
        configuration_commitment(&before).expect("before configuration"),
        configuration_commitment(&after).expect("after configuration")
    );
    assert_eq!(
        identity_commitment(
            &before,
            before_matrix,
            QualificationCacheLayer::CargoSources
        )
        .expect("before key identity"),
        identity_commitment(&after, after_matrix, QualificationCacheLayer::CargoSources)
            .expect("after key identity")
    );
}

fn plan_for_checkout(
    checkout: &str,
    run_id: u64,
    source_sha: String,
    input: &str,
    closure: &str,
) -> Plan {
    let run_key = run_key_for_ci(run_id, 1);
    let raw_id = package_id_for_checkout(checkout);
    Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan id"),
        base: None,
        head: source_sha.clone(),
        event: WorkflowEvent::Qualification,
        qualification: Some(qualification_context(run_id, source_sha)),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("qualification_bypass"))
            .expect("unavailable baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: vec![package_for_id(&raw_id)],
        obligations: vec![PlanObligation {
            task_id: "rust:demo:test".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "qualification_full".to_owned(),
            task_digest: digest_b3(b"task configuration"),
            input_digest: digest_b3(input.as_bytes()),
            closure_digest: digest_b3(closure.as_bytes()),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![matrix_for_id(&raw_id, input)],
        },
        task_ids: vec!["rust:demo:test".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn package_id_for_checkout(checkout: &str) -> String {
    format!("path+file://{checkout}/crates/demo#demo@0.1.0")
}

fn package_for_id(package_id: &str) -> PlanPackage {
    PlanPackage {
        package_id: package_id.to_owned(),
        name: "demo".to_owned(),
        manifest: "crates/demo/Cargo.toml".to_owned(),
        selected: true,
        reasons: vec!["changed".to_owned()],
        tasks: vec!["rust:demo:test".to_owned()],
    }
}

fn matrix_for_id(package_id: &str, input: &str) -> MatrixEntry {
    MatrixEntry {
        id: "stack:rust|task:demo-test".to_owned(),
        matrix_key: "matrix-demo-test".to_owned(),
        stack_id: "rust".to_owned(),
        task_id: "demo-test".to_owned(),
        lane_variant: None,
        run: "mise run test".to_owned(),
        task_digest: digest_b3(b"task configuration"),
        adapter_metadata: serde_json::json!({
            "package_id": package_id,
            "unit_id": package_id,
            "compile_driver": "cargo",
        }),
        execute_task_ids: ExecuteTaskIds::default(),
        input_digest: digest_b3(input.as_bytes()),
        report_id: "matrix-report-demo".to_owned(),
        job_id: "velnor-crate-demo".to_owned(),
        artifact_id: "velnor-crate-demo-a1".to_owned(),
        planned_platform: PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")
            .expect("planned platform"),
        cache_ids: Some(cache_ids()),
        declared_outputs: Vec::new(),
        test_run: Vec::new(),
    }
}

fn cache_ids() -> EntryCacheIds {
    EntryCacheIds::new(
        &digest_b3(b"workspace"),
        &digest_b3(b"lane"),
        &digest_b3(b"platform"),
        &digest_b3(b"toolchain"),
        &digest_b3(b"cache format"),
    )
    .expect("cache identity")
}

fn qualification_context(run_id: u64, source_sha: String) -> QualificationDispatch {
    QualificationDispatch {
        campaign: "identity-test".to_owned(),
        phase: QualificationPhase::UsefulDelta,
        repository: "tailrocks/velnor".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "tailrocks/velnor/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: source_sha.clone(),
        source_sha,
        run_id,
        run_attempt: 1,
        predecessor: Some(QualificationRunRef {
            run_id: run_id - 1,
            run_attempt: 1,
        }),
    }
}
