//! Merge-time revalidation tests.
//!
//! Declared via `#[path]` from `cover_revalidate.rs` under `cfg(test)`
//! so the revalidation module keeps its size gate. Builders live in
//! `cover_revalidate_fixtures.rs`, shared with the entry-validation
//! tests.

use super::cover_revalidate_fixtures::{NOW, anchored_verdict, manifest_for, plan_for, verdict};
use super::*;

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
    let mut hostile_ref = manifest.clone();
    hostile_ref.ref_ = "refs/heads/main;evil".to_owned();
    check("hostile branch ref", &plan, &hostile_ref);
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

/// Merge enforces the plan-time manifest invariants: every mutation
/// below re-binds the plan (`plan_for` over the mutated manifest) so the
/// manifest digest still verifies — only the named conjunct can fail.
/// An unbound forwarded proof rejected during planning must fail at merge too.
#[test]
fn merge_rejects_plan_rejected_manifest_invariants() {
    let commit = "a".repeat(40);
    let check = |label: &str, mutate: &dyn Fn(&mut BaselineManifest)| {
        let manifest = manifest_for(&commit);
        let mut mutated = manifest.clone();
        mutate(&mut mutated);
        let plan = plan_for(&mutated, Some(&commit));
        let (signals, miss) = verdict(&plan, Some(&mutated));
        assert!(signals.planning_failed, "{label}");
        assert!(miss.contains("cache_corrupt"), "{label}: {miss:?}");
    };
    check("forwarded proof run", &|m| m.tasks[0].proof_run_id = 123);
    check("foreign observed run", &|m| {
        m.tasks[0].observed_run_id = 456;
    });
    check("zero run id", &|m| m.run_id = 0);
    check("zero run attempt", &|m| m.run_attempt = 0);
    check("zero artifact id", &|m| m.artifact_id = 0);
    check("underived artifact name", &|m| {
        // Well-formed baseline grammar for the WRONG commit: passes the
        // proof constructor, must fail the derived-name conjunct.
        m.artifact_name = format!("velnor-baseline-{}-{}", "b".repeat(40), m.compatibility_id);
    });
    // Unverifiable generator with a matching plan pin: equality passes,
    // the verifiability conjunct must still fail.
    let mut manifest = manifest_for(&commit);
    manifest.generator_sha256 = "0".repeat(64);
    let mut plan = plan_for(&manifest, Some(&commit));
    plan.generator.sha256 = "0".repeat(64);
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(signals.planning_failed, "unverifiable generator");
    assert!(miss.contains("cache_corrupt"), "{miss:?}");
}

/// Expectations matching the `o/r` fixture manifest's anchors.
fn fixture_anchors() -> MergeAnchorExpectations {
    MergeAnchorExpectations {
        ci_strict_anchors: false,
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
            BTreeSet::from(["trust_scope_mismatch".to_owned()]),
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
    assert!(miss.contains("trust_scope_mismatch"), "{miss:?}");
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
    assert_eq!(miss, BTreeSet::from(["trust_scope_mismatch".to_owned()]));
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
            ci_strict_anchors: false,
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
        merge_anchors_from_parts(None, None, Some("refs/heads/main;evil"), None)
            .protected_ref
            .is_none(),
        "malformed pushed branch names yield no expectation"
    );
    assert!(
        merge_anchors_from_parts(None, Some("a b"), None, None)
            .protected_ref
            .is_none(),
        "malformed base branches yield no expectation"
    );
}

/// CI requires every anchor: any absent anchor fails closed instead
/// of skipping its check. Local runs (flag clear) keep the skip
/// behavior.
#[test]
fn ci_missing_any_anchor_fails_closed() {
    let commit = "a".repeat(40);
    let manifest = anchored_manifest(&commit);
    let plan = plan_for(&manifest, Some(&commit));
    let full = MergeAnchorExpectations {
        ci_strict_anchors: true,
        ..fixture_anchors()
    };
    let (signals, _) = anchored_verdict(&plan, Some(&manifest), &full);
    assert!(!signals.planning_failed, "full CI anchors pass");
    let mut drop_slug = full.clone();
    drop_slug.repository_slug = None;
    let mut drop_ref = full.clone();
    drop_ref.protected_ref = None;
    let mut drop_path = full;
    drop_path.workflow_path = None;
    for (label, anchors) in [
        ("repository", drop_slug),
        ("protected ref", drop_ref),
        ("workflow path", drop_path),
    ] {
        let (signals, miss) = anchored_verdict(&plan, Some(&manifest), &anchors);
        assert!(signals.planning_failed, "CI without {label} must fail");
        assert_eq!(miss, BTreeSet::from(["trust_scope_mismatch".to_owned()]));
    }
    let (signals, _) = verdict(&plan, Some(&manifest));
    assert!(
        !signals.planning_failed,
        "local runs still skip absent anchors"
    );
}

/// Merge refuses expired manifests even when the plan re-binds to
/// them, and accepts unexpired ones.
#[test]
fn merge_rejects_expired_manifest() {
    let commit = "a".repeat(40);
    let mut manifest = manifest_for(&commit);
    manifest.expires_at_unix = Some(NOW - 1);
    let plan = plan_for(&manifest, Some(&commit));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(signals.planning_failed, "expired manifest must fail");
    assert!(miss.contains("cache_corrupt"), "{miss:?}");
    manifest.expires_at_unix = Some(NOW + 10_000);
    let plan = plan_for(&manifest, Some(&commit));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(!signals.planning_failed, "{miss:?}");
}

/// Every token the anchor path emits validates against the contract vocabulary.
///
/// A novel token compiles and passes producer-side asserts but fails
/// `FinalReport` validation at merge, corrupting a `PlanningFailed`
/// verdict into Internal (CI once ran `foreign_anchor` into exactly
/// this). This test pins the producer to the contract's closed set.
#[test]
fn anchor_tokens_validate_against_contract_vocabulary() {
    use velnor_actions_contract::cachekey::validate_miss_reason;
    let commit = "a".repeat(40);
    let manifest = anchored_manifest(&commit);
    let plan = plan_for(&manifest, Some(&commit));
    let anchors = fixture_anchors();
    let mut forked = anchors.clone();
    forked.repository_slug = Some("evil/fork".to_owned());
    let mut other_ref = anchors.clone();
    other_ref.protected_ref = Some("refs/heads/other".to_owned());
    let mut other_path = anchors.clone();
    other_path.workflow_path = Some("other.yml".to_owned());
    let mut strict_missing = anchors.clone();
    strict_missing.ci_strict_anchors = true;
    strict_missing.workflow_path = None;
    for (label, anchors) in [
        ("foreign slug", forked),
        ("foreign ref", other_ref),
        ("foreign workflow path", other_path),
        ("strict missing anchor", strict_missing),
    ] {
        let (signals, miss) = anchored_verdict(&plan, Some(&manifest), &anchors);
        assert!(signals.planning_failed, "{label}");
        assert!(!miss.is_empty(), "{label}");
        for token in &miss {
            assert!(validate_miss_reason(token).is_ok(), "{label}: {token}");
        }
    }
}
