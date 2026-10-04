//! Source planner identities survive successful publication and live refinement.

use super::*;
use crate::cover_identity::cover_identity_fixtures::{
    discovery_with, inputs, provenance_for, seed_sources,
};
use crate::internal::plan_obligation::{GroupInputs, plan_group};
use crate::internal_plan::snapshot::ExecutionSnapshot;
use crate::internal_plan::wire_w2::GroupWire;
use velnor_actions_contract::{FinalReport, parse_strict_json};
use velnor_actions_mise::ToolCatalog;

const TASK: &str = "stack/rust/root/clippy/default";

fn source_plan(root: &Path, unknown: bool) -> (Plan, crate::discover::Discovery, ToolCatalog) {
    let member = root.join("member");
    fs::create_dir(&member).expect("member");
    seed_sources(&member);
    fs::write(root.join("README.md"), "before").expect("docs");
    if unknown {
        fs::write(
            member.join("src/lib.rs"),
            "pub fn f() { std::fs::read(\"data\"); }",
        )
        .expect("unknown inputs");
    }
    let mut discovery = discovery_with(&[TASK]);
    discovery.proposals[0].identity.unit_path = "member/Cargo.toml".to_owned();
    let catalog = ToolCatalog::pinned();
    let mut plan = fixture_plan(&"a".repeat(40), "r7-a1");
    plan.generator.sha256 = "1".repeat(64);
    let snapshot = ExecutionSnapshot::build(&discovery).with_checkout(root);
    let (obligation, entry) = plan_group(
        &GroupInputs {
            discovery: &discovery,
            task: &discovery.proposals[0],
            run_key: &plan.run_key,
            label: &plan.runner.label,
            catalog: &catalog,
            wire: GroupWire {
                event: plan.event,
                generator: &plan.generator,
            },
            changed: true,
            snapshot: &snapshot,
            root,
        },
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect("real source planner");
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    plan.task_ids = vec![TASK.to_owned()];
    plan.obligations = vec![obligation];
    plan.matrix.include = vec![entry];
    plan.validate().expect("real planned identity validates");
    (plan, discovery, catalog)
}

fn merge_source(plan: &Plan, temp: &Path, exit_code: i32) -> FinalReport {
    let run = temp.join("velnor").join(&plan.run_key);
    fs::create_dir_all(&run).expect("run");
    for (name, value) in [
        ("plan.json", serde_json::to_string(plan).expect("plan")),
        (
            "matrix.json",
            serde_json::to_string(&plan.matrix).expect("matrix"),
        ),
    ] {
        fs::write(run.join(name), value).expect("stage planner output");
    }
    for entry in &plan.matrix.include {
        crate::task_report::write_task_report_to(
            &plan.run_key,
            &entry.task_id,
            &entry.task_digest,
            exit_code,
            None,
            &[],
            temp,
        )
        .expect("execution task and matrix reports");
        let home = run
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        fs::create_dir_all(home.parent().expect("artifact root")).expect("artifact");
        fs::rename(run.join(&entry.matrix_key), home).expect("download execution artifact");
    }
    let job = &plan.obligations[0].job_id;
    let needs = serde_json::json!({"plan": "success", (job): "success"}).to_string();
    let inventory = serde_json::json!(["plan", job]).to_string();
    let request = crate::merge_request::assemble_with_needs(
        &plan.run_key,
        &run,
        Some(&needs),
        Some(&inventory),
        Some("push"),
        Some(&push_payload(&plan.head)),
    )
    .expect("assemble actual execution evidence and required jobs");
    let encoded = crate::merge::merge_internal(&request).expect("production final gate");
    let report: FinalReport =
        serde_json::from_value(parse_strict_json(&encoded).expect("strict final report"))
            .expect("final report");
    fs::write(run.join("final-report.json"), encoded).expect("stage gate-produced report");
    report
}

fn publish_source(plan: &Plan) -> BaselineManifest {
    let temp = tempfile::tempdir().expect("runner temp");
    let report = merge_source(plan, temp.path(), 0);
    assert_eq!(
        report.status,
        velnor_actions_contract::FinalStatus::Passed,
        "{:?}",
        report.miss_reasons
    );
    assert_eq!(
        report.counts.executed,
        u32::try_from(plan.obligations.len()).expect("count")
    );
    publish_fixture(&request_json(&plan.head), &plan.run_key, temp.path()).expect("publish");
    serde_json::from_value(staged_manifest(temp.path(), &plan.run_key)).expect("manifest")
}

#[test]
fn real_planner_report_publication_supports_docs_refinement() {
    for unknown in [false, true] {
        let root = tempfile::tempdir().expect("checkout");
        let (mut plan, discovery, catalog) = source_plan(root.path(), unknown);
        let manifest = publish_source(&plan);
        let proof = manifest.tasks[0]
            .proof
            .as_ref()
            .expect("direct proof mandatory");
        assert!(plan.obligations[0].execution_identity.matches_proof(proof));
        assert_eq!(proof.proof_run_id(), 7);
        fs::write(root.path().join("README.md"), "after").expect("docs edit");
        let changed = crate::select::ChangedSelection {
            affected: ["demo".to_owned()].into(),
            proof_refinable: ["demo".to_owned()].into(),
        };
        let covered = crate::cover_identity::apply_coverage(
            &mut plan,
            &manifest,
            &provenance_for(&manifest),
            &discovery,
            Some(&changed),
            &inputs(root.path(), &catalog),
        );
        assert_eq!(covered, u32::from(!unknown), "{:?}", plan.warnings);
        assert_eq!(
            plan.obligations[0].decision,
            if unknown {
                ObligationDecision::Execute
            } else {
                ObligationDecision::CoveredByTrustedBaseline
            }
        );
    }
}

#[test]
fn retry_requires_every_execution_dimension_and_structured_proof() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a1");
    let manifest = publish_source(&plan);
    let request = publish_request(&request_json(&plan.head)).expect("request");
    evidence::qualify_existing(&request, &plan, &manifest, 7, 2).expect("exact retry");
    for dimension in 0..5 {
        let mut changed = plan.clone();
        let identity = &plan.obligations[0].execution_identity;
        let mut fields = [
            identity.graph_digest().to_owned(),
            identity.toolchain_id().to_owned(),
            identity.mbx_digest().to_owned(),
            identity.platform_id().to_owned(),
            identity.profile().to_owned(),
        ];
        fields[dimension] = if dimension == 4 {
            "different".to_owned()
        } else {
            digest(99)
        };
        changed.obligations[0].execution_identity =
            velnor_actions_contract::TaskExecutionIdentity::new(
                &fields[0], &fields[1], &fields[2], &fields[3], &fields[4],
            )
            .expect("different identity");
        assert!(evidence::qualify_existing(&request, &changed, &manifest, 7, 2).is_err());
    }
    let mut missing = manifest;
    missing.tasks[0].proof = None;
    assert!(evidence::qualify_existing(&request, &plan, &missing, 7, 2).is_err());
}

#[test]
fn actual_failed_gate_never_mints_execution_proof() {
    let root = tempfile::tempdir().expect("checkout");
    let (plan, _, _) = source_plan(root.path(), false);
    let temp = tempfile::tempdir().expect("runner temp");
    let report = merge_source(&plan, temp.path(), 1);
    assert_eq!(report.status, velnor_actions_contract::FinalStatus::Failed);
    assert!(publish_fixture(&request_json(&plan.head), &plan.run_key, temp.path()).is_err());
    assert!(
        !temp
            .path()
            .join("velnor")
            .join(&plan.run_key)
            .join("published/baseline.json")
            .exists()
    );
}
