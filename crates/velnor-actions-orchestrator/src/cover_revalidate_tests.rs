//! Merge-time revalidation tests.
//!
//! Declared via `#[path]` from `cover_revalidate.rs` under `cfg(test)`
//! so the revalidation module keeps its size gate.

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

/// Revalidation verdict for one plan/manifest pair without anchors.
///
/// Explicit empty anchors keep these hermetic: the env-reading
/// production entry would compare against CI ground truth instead.
fn verdict(plan: &Plan, manifest: Option<&BaselineManifest>) -> (Signals, BTreeSet<String>) {
    anchored_verdict(plan, manifest, &MergeAnchorExpectations::default())
}

/// Revalidation verdict for one plan/manifest/anchors triple.
fn anchored_verdict(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    anchors: &MergeAnchorExpectations,
) -> (Signals, BTreeSet<String>) {
    let mut signals = Signals::default();
    let mut miss = BTreeSet::new();
    revalidate_coverage_with_anchors(plan, manifest, &mut signals, &mut miss, anchors);
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

/// Expectations matching the `o/r` fixture manifest's anchors.
fn fixture_anchors() -> MergeAnchorExpectations {
    MergeAnchorExpectations {
        repository_slug: Some("o/r".to_owned()),
        protected_ref: Some("refs/heads/testmain".to_owned()),
        workflow_path: Some(".github/workflows/ci.yml".to_owned()),
    }
}

/// Fixture manifest bound to the `github.com/o/r` anchor.
fn anchored_manifest(commit: &str) -> BaselineManifest {
    let mut manifest = manifest_for(commit);
    manifest.repository_id = digest_b3("github.com/o/r".as_bytes());
    manifest
}

/// Environment anchors pass a matching manifest and refuse every
/// foreign field with its own token; a self-consistent foreign pair
/// that matches its plan still fails.
#[test]
fn env_anchors_match_and_refuse_foreign() {
    let commit = "a".repeat(40);
    let manifest = anchored_manifest(&commit);
    let plan = plan_for(&manifest, Some(&commit));
    let anchors = fixture_anchors();
    let (signals, miss) = anchored_verdict(&plan, Some(&manifest), &anchors);
    assert!(!signals.planning_failed, "{miss:?}");
    assert!(miss.is_empty());
    let check = |label: &str, manifest: &BaselineManifest, anchors: &MergeAnchorExpectations| {
        let (signals, miss) = anchored_verdict(&plan, Some(manifest), anchors);
        assert!(signals.planning_failed, "{label}");
        assert_eq!(
            miss,
            BTreeSet::from(["foreign_anchor".to_owned()]),
            "{label}"
        );
    };
    let mut forked = anchors.clone();
    forked.repository_slug = Some("evil/fork".to_owned());
    check("foreign slug", &manifest, &forked);
    let unanchored = manifest_for(&commit);
    let unanchored_plan = plan_for(&unanchored, Some(&commit));
    let (signals, miss) = anchored_verdict(&unanchored_plan, Some(&unanchored), &anchors);
    assert!(signals.planning_failed, "unbound repository id");
    assert!(miss.contains("foreign_anchor"), "{miss:?}");
    let mut other_ref = anchors.clone();
    other_ref.protected_ref = Some("refs/heads/other".to_owned());
    check("foreign ref", &manifest, &other_ref);
    let mut other_path = anchors.clone();
    other_path.workflow_path = Some("other.yml".to_owned());
    check("foreign workflow path", &manifest, &other_path);
    // Attack pair: a fork manifest self-consistent with its own plan
    // passes every plan check, so only the env anchors refuse it.
    let mut evil = anchored_manifest(&commit);
    evil.repository_id = digest_b3("github.com/evil/fork".as_bytes());
    evil.workflow_ref = "evil/fork/.github/workflows/ci.yml@refs/heads/testmain".to_owned();
    let evil_plan = plan_for(&evil, Some(&commit));
    let (signals, _) = verdict(&evil_plan, Some(&evil));
    assert!(!signals.planning_failed, "plan checks alone pass the pair");
    let (signals, miss) = anchored_verdict(&evil_plan, Some(&evil), &anchors);
    assert!(signals.planning_failed, "self-consistent foreign pair");
    assert_eq!(miss, BTreeSet::from(["foreign_anchor".to_owned()]));
}

/// Environment values map to anchor expectations: base branch and push
/// refs name the protected ref, pull refs never do, and malformed
/// values yield no expectation instead of guesses.
#[test]
fn anchor_parts_map_env_values() {
    assert_eq!(
        merge_anchors_from_parts(
            Some("O/R"),
            Some("testmain"),
            None,
            Some("o/r/.github/workflows/ci.yml@refs/heads/testmain"),
        ),
        fixture_anchors(),
    );
    assert_eq!(
        merge_anchors_from_parts(None, None, Some("refs/heads/testmain"), None),
        MergeAnchorExpectations {
            repository_slug: None,
            protected_ref: Some("refs/heads/testmain".to_owned()),
            workflow_path: None,
        },
        "push jobs read the protected ref from GITHUB_REF"
    );
    assert!(
        merge_anchors_from_parts(None, None, Some("refs/pull/1/merge"), None)
            .protected_ref
            .is_none(),
        "pull refs never become protected expectations"
    );
    assert_eq!(
        merge_anchors_from_parts(Some("nope"), Some(""), Some("nope"), Some("nope")),
        MergeAnchorExpectations::default(),
        "malformed values yield no expectations"
    );
    assert!(
        merge_anchors_from_parts(None, Some("a b"), None, None)
            .protected_ref
            .is_none(),
        "malformed base branches yield no expectation"
    );
}
