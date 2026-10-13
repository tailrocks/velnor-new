//! Merge-time coverage revalidation against the trusted manifest.
//!
//! Declared via `#[path]` from `cover.rs` (no `lib.rs` edit).

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ObligationDecision, Plan, canonical_json_bytes, digest_b3, validate_digest,
};

use crate::cover::Signals;
use crate::cover_baseline::provenance_check::parse_workflow_ref;
use crate::merge::BaselineManifest;

/// Revalidate planner coverage claims against the trusted manifest.
///
/// Every failure carries a miss token so diagnostics never emit bare
/// `planning_failed` verdicts. Beyond digest binding, the manifest's
/// provenance must match plan-anchored expectations: the source commit
/// must equal the plan base, the generator must equal the plan
/// generator, the trusted push/passed invariant must hold, and ref,
/// workflow-ref, and repository shapes must be well-formed and
/// internally consistent. The disposition match stays exhaustive so a
/// future decision variant fails to compile here instead of silently
/// skipping revalidation.
///
/// Merge consumes the plan only: full repo/ref/workflow anchor
/// comparison needs checkout ground truth the merge never rediscovers,
/// so well-formed-but-foreign anchors bind at plan time instead (see
/// `validate_provenance`); malformed or internally inconsistent
/// anchors fail here.
pub(crate) fn revalidate_coverage(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let mut covered = Vec::new();
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::CoveredByTrustedBaseline => covered.push(obligation),
            ObligationDecision::Execute | ObligationDecision::ReusedFromTaskCache => {}
        }
    }
    if covered.is_empty() {
        return;
    }
    let Some(manifest) = manifest else {
        signals.planning_failed = true;
        miss_reasons.insert("source_missing".to_owned());
        return;
    };
    if manifest.schema != crate::internal_plan::snapshot::CANONICAL_SCHEMA_VERSION {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
        return;
    }
    if !manifest_provenance_matches_plan(plan, manifest) {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
        return;
    }
    for obligation in covered {
        let Some(proof) = &obligation.baseline_proof else {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        let hit = manifest
            .tasks
            .iter()
            .find(|task| task.task_id == obligation.task_id);
        let Some(task) = hit else {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        let bound = task.task_digest == obligation.task_digest
            && task.input_digest == obligation.input_digest
            && task.closure_digest == obligation.closure_digest
            && proof.run_id() == task.proof_run_id
            && proof.source_commit() == manifest.source_commit
            && proof.artifact_id() == manifest.artifact_id
            && proof.artifact_name() == manifest.artifact_name;
        // A serialization failure is planning_failed, never a digest over an
        // empty default that could verify against a forged proof.
        match canonical_json_bytes(manifest) {
            Ok(bytes) if bound && proof.manifest_digest() == digest_b3(&bytes) => {}
            _ => {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
        }
    }
}

/// Plan-anchored provenance: base, generator, trusted invariant, shapes.
///
/// A covered plan without a base can prove nothing; every other field
/// compares against the plan value or the trusted invariant.
fn manifest_provenance_matches_plan(plan: &Plan, manifest: &BaselineManifest) -> bool {
    let Some(base) = plan.base.as_deref() else {
        return false;
    };
    manifest.source_commit == base
        && manifest.generator_version == plan.generator.version
        && manifest.generator_sha256 == plan.generator.sha256
        && manifest.event == "push"
        && manifest.final_status == "passed"
        && validate_digest(&manifest.repository_id).is_ok()
        && ref_shape_ok(&manifest.ref_)
        && workflow_ref_consistent(manifest)
}

/// Protected-branch ref shape: `refs/heads/<nonempty branch>`.
fn ref_shape_ok(git_ref: &str) -> bool {
    git_ref.starts_with("refs/heads/")
        && git_ref.len() > "refs/heads/".len()
        && !git_ref.contains(' ')
}

/// The workflow ref parses and agrees with the manifest's own ref.
fn workflow_ref_consistent(manifest: &BaselineManifest) -> bool {
    let Some((_, _, git_ref)) = parse_workflow_ref(&manifest.workflow_ref) else {
        return false;
    };
    git_ref == manifest.ref_
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::{
        BaselineProof, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
        RunnerSelection, Trust, WorkflowEvent,
    };

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
            compatibility_id: digest_b3(b"compat"),
            artifact_id: 9,
            artifact_name: "velnor-plan-local".to_owned(),
            tasks: vec![crate::merge::required_evidence::BaselineTaskEntry {
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
            9,
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

    /// Revalidation verdict for one plan/manifest pair.
    fn verdict(plan: &Plan, manifest: Option<&BaselineManifest>) -> (Signals, BTreeSet<String>) {
        let mut signals = Signals::default();
        let mut miss = BTreeSet::new();
        revalidate_coverage(plan, manifest, &mut signals, &mut miss);
        (signals, miss)
    }

    #[test]
    fn bound_coverage_revalidates() {
        let commit = "a".repeat(40);
        let manifest = manifest_for(&commit);
        let plan = plan_for(&manifest, Some(&commit));
        let (signals, miss) = verdict(&plan, Some(&manifest));
        assert!(!signals.planning_failed);
        assert!(miss.is_empty());
    }

    #[test]
    fn wrong_provenance_fails_per_field() {
        let commit = "a".repeat(40);
        let manifest = manifest_for(&commit);
        let plan = plan_for(&manifest, Some(&commit));
        let check = |label: &str, plan: &Plan, manifest: &BaselineManifest| {
            let (signals, miss) = verdict(plan, Some(manifest));
            assert!(signals.planning_failed, "{label}");
            assert!(miss.contains("cache_corrupt"), "{label}: {miss:?}");
        };
        let other_base = plan_for(&manifest, Some(&"b".repeat(40)));
        check("wrong base", &other_base, &manifest);
        let no_base = plan_for(&manifest, None);
        check("missing base", &no_base, &manifest);
        let mut generator = manifest.clone();
        generator.generator_version = "9.9.9".to_owned();
        check("wrong generator version", &plan, &generator);
        let mut generator = manifest.clone();
        generator.generator_sha256 = "f".repeat(64);
        check("wrong generator sha", &plan, &generator);
        let mut event = manifest.clone();
        event.event = "pull_request".to_owned();
        check("wrong event", &plan, &event);
        let mut status = manifest.clone();
        status.final_status = "failed".to_owned();
        check("failed status", &plan, &status);
        let mut repo = manifest.clone();
        repo.repository_id = "bogus".to_owned();
        check("malformed repository", &plan, &repo);
        let mut git_ref = manifest.clone();
        git_ref.ref_ = "testmain".to_owned();
        check("malformed ref", &plan, &git_ref);
        let mut workflow = manifest.clone();
        workflow.workflow_ref = "o/r/.github/workflows/ci.yml@refs/heads/other".to_owned();
        check("inconsistent workflow ref", &plan, &workflow);
        let mut workflow = manifest.clone();
        workflow.workflow_ref = "not-a-ref".to_owned();
        check("unparsable workflow ref", &plan, &workflow);
        // A manifest moved to another commit matches neither the plan
        // base nor the proof binding.
        let mut moved = manifest.clone();
        moved.source_commit = "b".repeat(40);
        check("moved commit", &plan, &moved);
        let (signals, _) = verdict(&plan, None);
        assert!(signals.planning_failed, "missing manifest");
    }
}
