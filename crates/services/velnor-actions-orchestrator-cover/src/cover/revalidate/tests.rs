use super::super::Signals;
use super::*;
use super::{MergeAnchorExpectations, revalidate_coverage_with_anchors};
use std::collections::BTreeSet;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    BaselineProof, ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix,
    PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_merge_ports::BaselineManifest;

/// Revalidation verdict for one plan/manifest pair without anchors.
///
/// Explicit empty anchors keep these hermetic: the env-reading
/// production entry would compare against CI ground truth instead.
/// Fixed merge-time clock: fixtures without expiry pass at any `now`.
pub(crate) const NOW: u64 = 1_800_000_000;
/// Revalidation verdict for one plan/manifest/anchors triple.
pub(crate) fn anchored_verdict(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    anchors: &MergeAnchorExpectations,
) -> (Signals, BTreeSet<String>) {
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    revalidate_coverage_with_anchors(plan, manifest, &mut signals, &mut miss, anchors, NOW);
    (signals, miss)
}
/// Trusted manifest with one entry over `commit`.
pub(crate) fn manifest_for(commit: &str) -> BaselineManifest {
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
        tasks: vec![velnor_actions_orchestrator_merge_ports::BaselineTaskEntry {
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
pub(crate) fn plan_for(manifest: &BaselineManifest, base: Option<&str>) -> Plan {
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
    }
}
pub(crate) fn verdict(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
) -> (Signals, BTreeSet<String>) {
    anchored_verdict(plan, manifest, &MergeAnchorExpectations::default())
}

/// Task and input digests shared by obligation and entry.
pub(crate) fn digests() -> (String, String, String) {
    (
        digest_b3(b"task"),
        digest_b3(b"inputs"),
        digest_b3(b"closure"),
    )
}

mod cover_revalidate_entry_tests;
mod cover_revalidate_fixtures;
mod cover_revalidate_tests;
