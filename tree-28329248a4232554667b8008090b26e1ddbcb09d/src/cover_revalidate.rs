//! Merge-time coverage revalidation against the trusted manifest.
//!
//! Declared via `#[path]` from `cover.rs` (no `lib.rs` edit).

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ObligationDecision, Plan, canonical_json_bytes, digest_b3, validate_digest,
};

use crate::cover::Signals;
use crate::cover_baseline::provenance_check::{
    baseline_artifact_name, is_unverifiable_generator_sha, parse_workflow_ref, task_run_ids_bound,
    validate_task_entry,
};
use crate::cover_baseline::unix_now;
use crate::decisions::baseline_expired;
use crate::merge::BaselineManifest;

/// Merge-time anchor expectations from runner-owned environment.
///
/// Each field compares the manifest's anchors against CI ground truth
/// instead of the possibly attacker-influenced plan: an evil-fork plan
/// paired with a self-consistent evil manifest fails here even though
/// it matches the plan. `None` fields skip their check (local runs
/// without CI env), except when `ci_strict_anchors` is set: CI always
/// provides the repository, protected ref, and workflow path, so any
/// missing anchor there fails closed instead of skipping. The
/// plan-anchored invariants below still apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MergeAnchorExpectations {
    /// Lowercase `owner/repo` slug from `GITHUB_REPOSITORY`.
    pub(crate) repository_slug: Option<String>,
    /// Protected ref from `GITHUB_BASE_REF` or `GITHUB_REF`.
    pub(crate) protected_ref: Option<String>,
    /// Generated workflow path from `GITHUB_WORKFLOW_REF`.
    pub(crate) workflow_path: Option<String>,
    /// Fail closed when any anchor is absent (CI only).
    pub(crate) ci_strict_anchors: bool,
}

/// Anchor expectations from the runner-owned environment.
///
/// `GITHUB_REPOSITORY` names the repo slug; `GITHUB_BASE_REF` (pull
/// requests) or a `refs/heads/` `GITHUB_REF` (pushes) names the
/// protected ref; `GITHUB_WORKFLOW_REF` (`<repo>/<path>@<ref>`) names
/// the workflow path. Malformed values yield `None`, never guesses.
/// Under `GITHUB_ACTIONS` every anchor is mandatory: the runner always
/// sets them, so absence means tampering, never a local run.
pub(crate) fn merge_anchors_from_env() -> MergeAnchorExpectations {
    let mut anchors = merge_anchors_from_parts(
        std::env::var("GITHUB_REPOSITORY").ok().as_deref(),
        std::env::var("GITHUB_BASE_REF").ok().as_deref(),
        std::env::var("GITHUB_REF").ok().as_deref(),
        std::env::var("GITHUB_WORKFLOW_REF").ok().as_deref(),
    );
    anchors.ci_strict_anchors = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    anchors
}

/// Anchor expectations from explicit environment values.
///
/// Pure so tests cover the mapping hermetically; [`merge_anchors_from_env`]
/// is the thin production reader over this function.
fn merge_anchors_from_parts(
    repository: Option<&str>,
    base_ref: Option<&str>,
    git_ref: Option<&str>,
    workflow_ref: Option<&str>,
) -> MergeAnchorExpectations {
    MergeAnchorExpectations {
        repository_slug: repository.and_then(crate::origin::validate_repository_slug),
        protected_ref: protected_ref_from(base_ref, git_ref),
        workflow_path: workflow_ref
            .and_then(parse_workflow_ref)
            .map(|(_, path, _)| path),
        ci_strict_anchors: false,
    }
}

/// Protected ref from the base branch or the current ref.
///
/// Pull-request and merge-group jobs read `GITHUB_BASE_REF` (a short
/// branch name); push jobs fall back to `GITHUB_REF` when it already
/// names a protected branch ref. Anything else yields no expectation.
fn protected_ref_from(base_ref: Option<&str>, git_ref: Option<&str>) -> Option<String> {
    if let Some(base) = base_ref.filter(|base| valid_branch_name(base)) {
        return Some(format!("refs/heads/{base}"));
    }
    git_ref
        .filter(|git_ref| {
            git_ref.starts_with("refs/heads/")
                && git_ref.len() > "refs/heads/".len()
                && !git_ref.chars().any(char::is_whitespace)
        })
        .map(str::to_owned)
}

/// True for plausible branch names: nonempty, no whitespace, no
/// traversal, not HEAD.
fn valid_branch_name(base: &str) -> bool {
    !base.is_empty()
        && !base.chars().any(char::is_whitespace)
        && !base.contains("..")
        && base != "HEAD"
}

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
/// Anchors additionally compare against runner-owned environment (see
/// [`MergeAnchorExpectations`]), so a well-formed-but-foreign plan and
/// manifest pair fails here instead of passing on internal consistency
/// plus plan agreement alone.
pub(crate) fn revalidate_coverage(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    revalidate_coverage_with_anchors(
        plan,
        manifest,
        signals,
        miss_reasons,
        &merge_anchors_from_env(),
        unix_now(),
    );
}

/// Revalidate coverage against explicit anchor expectations.
///
/// Pure over `anchors` and `now_unix` so tests cover every field
/// hermetically; [`revalidate_coverage`] is the production reader over
/// this function.
pub(crate) fn revalidate_coverage_with_anchors(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
    anchors: &MergeAnchorExpectations,
    now_unix: u64,
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
    if !manifest_provenance_matches_plan(plan, manifest, now_unix) {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
        return;
    }
    if !merge_anchors_match(manifest, anchors) {
        signals.planning_failed = true;
        // Contract cache §3 vocabulary: a manifest whose anchors
        // disagree with runner ground truth is a trust-scope mismatch,
        // never a novel token (an unlisted token fails FinalReport
        // validation and corrupts the verdict into Internal).
        miss_reasons.insert("trust_scope_mismatch".to_owned());
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
        // Advisory gate mirrors plan-time `gate_guards` per obligation:
        // the planner reruns (never covers) an advisory task without
        // fresh external data, so a covered claim here proves a forged
        // plan, not a stale-but-honest one.
        if crate::external_data::external_data_kind(&task.task_id).is_some()
            && !crate::external_data::may_skip_external_data(
                true,
                task.external_data.as_ref(),
                crate::external_data::DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS,
            )
        {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
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
/// compares against the plan value or the trusted invariant. Mirrors the
/// plan-time [`validate_provenance`](crate::cover_baseline::provenance_check::validate_provenance)
/// manifest checks (identified run, verifiable generator, derived
/// artifact name, per-task entry validation, freshness) so a manifest
/// the plan rejects can never pass at merge. Per-task entry validation
/// is the shared [`validate_task_entry`](crate::cover_baseline::provenance_check::validate_task_entry):
/// identity shapes, run binding, structured-proof match, and
/// external-data validity are identical on both sides by
/// construction, not by parallel reimplementation. Advisory
/// presence/freshness mirrors separately per obligation below (the
/// planner gates it at coverage time, not validation time).
///
/// `source_commit` binds to `plan.base`, not to a checkout: the merge
/// job renders no checkout step, and no CI variable carries the base
/// SHA, so checkout-anchoring is infeasible here. Replay is still
/// constrained: per-obligation digests bind every covered task to
/// byte-identical inputs, and expiry below refuses stale manifests.
fn manifest_provenance_matches_plan(
    plan: &Plan,
    manifest: &BaselineManifest,
    now_unix: u64,
) -> bool {
    let Some(base) = plan.base.as_deref() else {
        return false;
    };
    if baseline_expired(manifest.expires_at_unix, now_unix) {
        return false;
    }
    let identified = manifest.run_id > 0 && manifest.run_attempt > 0 && manifest.artifact_id > 0;
    let derived_name = baseline_artifact_name(&manifest.source_commit, &manifest.compatibility_id)
        .is_ok_and(|expect| manifest.artifact_name == expect);
    let derived_id = manifest.artifact_id
        == crate::cover_compat::baseline_artifact_numeric_id(&manifest.artifact_name);
    let run_bound = manifest
        .tasks
        .iter()
        .all(|task| task_run_ids_bound(task, manifest.run_id));
    let entries_ok = manifest
        .tasks
        .iter()
        .all(|task| validate_task_entry(task, manifest.run_id).is_ok());
    manifest.source_commit == base
        && manifest.generator_version == plan.generator.version
        && manifest.generator_sha256 == plan.generator.sha256
        && !is_unverifiable_generator_sha(&manifest.generator_sha256)
        && manifest.event == "push"
        && manifest.final_status == "passed"
        && identified
        && derived_name
        && derived_id
        && run_bound
        && entries_ok
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

/// Manifest anchors against runner-owned expectations.
///
/// Every present field must match: the repository slug binds both the
/// manifest's repository id and its workflow-ref slug, the protected
/// ref binds the manifest ref, and the workflow path binds the
/// workflow-ref path. Absent fields skip; a plan-consistent but
/// environment-foreign manifest fails.
fn merge_anchors_match(manifest: &BaselineManifest, anchors: &MergeAnchorExpectations) -> bool {
    if anchors.ci_strict_anchors
        && (anchors.repository_slug.is_none()
            || anchors.protected_ref.is_none()
            || anchors.workflow_path.is_none())
    {
        return false;
    }
    if let Some(slug) = anchors.repository_slug.as_deref() {
        let bound = digest_b3(format!("github.com/{slug}").as_bytes());
        if manifest.repository_id != bound {
            return false;
        }
        let slug_ok = parse_workflow_ref(&manifest.workflow_ref)
            .is_some_and(|(carried, _, _)| carried.to_lowercase() == slug);
        if !slug_ok {
            return false;
        }
    }
    if anchors
        .protected_ref
        .as_deref()
        .is_some_and(|expected| manifest.ref_ != expected)
    {
        return false;
    }
    if let Some(expected) = anchors.workflow_path.as_deref() {
        let path_ok =
            parse_workflow_ref(&manifest.workflow_ref).is_some_and(|(_, path, _)| path == expected);
        if !path_ok {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "cover_revalidate_entry_tests.rs"]
mod cover_revalidate_entry_tests;
#[cfg(test)]
#[path = "cover_revalidate_fixtures.rs"]
pub(crate) mod cover_revalidate_fixtures;
#[cfg(test)]
#[path = "cover_revalidate_tests.rs"]
mod cover_revalidate_tests;
