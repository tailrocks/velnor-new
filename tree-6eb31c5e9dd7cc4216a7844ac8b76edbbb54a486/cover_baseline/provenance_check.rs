//! Validated baseline provenance: expected values, never bare syntax.
//!
//! Declared via `#[path]` from `cover_baseline.rs` (no `lib.rs` edit).
//! A [`ValidatedProvenance`] binds the actual repository, protected
//! workflow/ref/event, exact base commit, successful run/attempt,
//! artifact name plus service ID, manifest digest, schema version, and
//! generator plus task compatibility.

use velnor_actions_contract::{digest_b3, validate_digest};

use crate::merge::BaselineManifest;
use velnor_actions_orchestrator_graph::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA;

// Unit tests live here so `provenance_check.rs` keeps its size gate.
#[cfg(test)]
mod tests;
// Slug and forwarded-proof tests live apart for the same reason.
/// Expected provenance values the manifest must match exactly.
#[derive(Debug, Clone)]
pub(crate) struct ProvenanceExpectations {
    /// Exact base commit (40 lowercase hex).
    pub(crate) base: String,
    /// Protected default-branch name.
    pub(crate) branch: String,
    /// Generated workflow path.
    pub(crate) workflow_path: String,
    /// Exact generator version.
    pub(crate) generator_version: String,
    /// Exact generator SHA.
    pub(crate) generator_sha256: String,
    /// Repository identity digest, when a git origin anchors it.
    pub(crate) repository_id: Option<String>,
    /// Lowercase `owner/repo` slug from the git origin, when the origin
    /// is a `github.com` remote the workflow slug can name.
    pub(crate) repository_slug: Option<String>,
    /// True when the runner-owned env slug disagrees with the git
    /// origin: validation fails closed, never picks a side.
    pub(crate) repository_conflict: bool,
}

/// Provenance validated against [`ProvenanceExpectations`].
///
/// Only validated values construct coverage proofs and baseline records;
/// validated-but-uncarried dimensions (event, ref, workflow, attempt,
/// schema, generator, repository) are enforced by validation.
/// Forwarded proof runs (originating run differs from the carrying run)
/// fail validation outright: the manifest cannot prove the originating
/// run succeeded, and warn-and-cover would grant coverage for success
/// nobody attested.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedProvenance {
    /// Exact trusted source commit.
    pub(crate) source_commit: String,
    /// Proof run ID.
    pub(crate) run_id: u64,
    /// Derived baseline artifact name.
    pub(crate) artifact_name: String,
    /// Manifest-assigned numeric fingerprint of the artifact name.
    ///
    /// Never the service-assigned artifact ID: the publisher writes the
    /// manifest before uploading, so no manifest can carry an ID the
    /// service assigns at upload time.
    pub(crate) artifact_id: u64,
    /// Manifest content digest.
    pub(crate) manifest_digest: String,
}

/// Derive `velnor-baseline-<commit>-<compat>` with full IDs.
///
/// # Errors
///
/// Returns the reason when the commit or compat ID is malformed.
pub(crate) fn baseline_artifact_name(commit: &str, compat: &str) -> Result<String, String> {
    let lower_hex = |byte: &u8| matches!(byte, b'0'..=b'9' | b'a'..=b'f');
    let sha = commit.len() == 40 && commit.bytes().all(|byte| lower_hex(&byte));
    reject(sha, "bad_source_commit")?;
    validate_digest(compat).map_err(|_| "bad_compatibility_id".to_owned())?;
    Ok(format!("velnor-baseline-{commit}-{compat}"))
}

/// True only for protected pushes; PR/fork/merge-group runs never publish.
pub(crate) fn publish_event_eligible(
    event: velnor_actions_contract_workflow::WorkflowEvent,
) -> bool {
    event == velnor_actions_contract_workflow::WorkflowEvent::Push
}

/// Reject a failed evidence check with its reason.
fn reject(ok: bool, reason: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(reason.to_owned()) }
}

/// Carried proof binds the task entry's identity and originating run.
///
/// Compared field by field over validated getters: a carried proof that
/// no longer matches its entry's identity is a mismatch, never a pass.
fn proof_matches_task(
    proof: &velnor_actions_contract_workflow::ManifestTaskProof,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
) -> bool {
    proof.task_id() == task.task_id
        && proof.task_digest() == task.task_digest
        && proof.input_digest() == task.input_digest
        && proof.proof_run_id() == task.proof_run_id
}

/// Validate evidence against expected values: repo, workflow, ref,
/// event, base, run/attempt, artifact, manifest, schema, generator.
///
/// Every comparison is manifest-against-expectation; manifest-internal
/// consistency alone never validates.
/// # Errors
///
/// Returns the first failing dimension's reason.
pub(crate) fn validate_provenance(
    manifest: &BaselineManifest,
    manifest_digest: &str,
    expected: &ProvenanceExpectations,
) -> Result<ValidatedProvenance, String> {
    let expect = baseline_artifact_name(&manifest.source_commit, &manifest.compatibility_id)?;
    let identified = manifest.run_id > 0 && manifest.run_attempt > 0 && manifest.artifact_id > 0;
    let trusted = manifest.event == "push" && manifest.final_status == "passed";
    let generated = manifest.generator_version == expected.generator_version
        && manifest.generator_sha256 == expected.generator_sha256;
    if velnor_actions_orchestrator_graph::internal_plan::snapshot::check_canonical_version(
        manifest.schema,
    )
    .is_err()
    {
        return Err(format!(
            "stale_schema:migration_required:v{}",
            manifest.schema
        ));
    }
    reject(manifest.source_commit == expected.base, "wrong_commit")?;
    reject(
        manifest.ref_ == format!("refs/heads/{}", expected.branch),
        "wrong_ref",
    )?;
    reject(trusted, "untrusted_proof")?;
    reject(identified, "bad_proof_identity")?;
    reject(
        !is_unverifiable_generator_sha(&manifest.generator_sha256),
        "generator_unverifiable",
    )?;
    reject(generated, "generator_mismatch")?;
    validate_repository(manifest, expected)?;
    validate_workflow_ref(manifest, expected)?;
    reject(manifest.artifact_name == expect, "artifact_mismatch")?;
    reject(
        manifest.artifact_id
            == crate::cover_compat::baseline_artifact_numeric_id(&manifest.artifact_name),
        "artifact_mismatch",
    )?;
    for task in &manifest.tasks {
        validate_task_entry(task, manifest.run_id)?;
    }
    let bound = manifest
        .tasks
        .iter()
        .all(|task| task_run_ids_bound(task, manifest.run_id));
    reject(bound, "originating_run_unverified")?;
    Ok(ValidatedProvenance {
        source_commit: manifest.source_commit.clone(),
        run_id: manifest.run_id,
        artifact_name: manifest.artifact_name.clone(),
        artifact_id: manifest.artifact_id,
        manifest_digest: manifest_digest.to_owned(),
    })
}

/// Shared run-binding predicate: a task entry is bound to the carrying
/// manifest's own run only. Plan-time [`validate_provenance`] and
/// merge-time revalidation call this one predicate so a forwarded proof
/// the plan rejects can never pass at merge.
pub(crate) fn task_run_ids_bound(
    task: &crate::merge::required_evidence::BaselineTaskEntry,
    run_id: u64,
) -> bool {
    run_id > 0 && task.proof_run_id == run_id && task.observed_run_id == run_id
}

/// Validate one task entry: identities, run binding, freshness, proof.
///
/// Every entry passes every check: a structured proof adds its binding
/// check on top instead of replacing the identity, run, and freshness
/// checks. The observing run must be the carrying manifest's own run:
/// an entry "observed" by any other run is a carried-proof identity
/// mismatch. A same-run proof run is success-bound by the manifest's
/// own trusted checks; a forwarded proof run fails the manifest in
/// [`validate_provenance`], never warn-and-cover.
/// # Errors
///
/// Returns the first failing check's reason.
pub(crate) fn validate_task_entry(
    task: &crate::merge::required_evidence::BaselineTaskEntry,
    run_id: u64,
) -> Result<(), String> {
    let ids_ok = velnor_actions_contract::validate_task_id(&task.task_id).is_ok()
        && validate_digest(&task.task_digest).is_ok()
        && validate_digest(&task.input_digest).is_ok()
        && validate_digest(&task.closure_digest).is_ok();
    reject(ids_ok, "bad_task_identity")?;
    reject(
        task.proof_run_id > 0 && task.observed_run_id > 0,
        "bad_proof_identity",
    )?;
    reject(task.observed_run_id == run_id, "proof_mismatch")?;
    let fresh_ok = task
        .external_data
        .as_ref()
        .is_none_or(|proof| proof.validate().is_ok());
    reject(fresh_ok, "bad_external_data")?;
    if let Some(proof) = &task.proof {
        proof.validate().map_err(|_| "bad_task_proof".to_owned())?;
        reject(proof_matches_task(proof, task), "proof_mismatch")?;
    }
    Ok(())
}

/// True for generator SHAs no manifest may bind: empty, all-zero, or the
/// explicit unresolved marker. Shared with merge-time revalidation.
pub(crate) fn is_unverifiable_generator_sha(sha: &str) -> bool {
    sha.is_empty()
        || sha == UNRESOLVED_GENERATOR_SHA
        || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// Repository check: exact match against the expected anchor.
///
/// A conflicted expectation (env slug disagrees with the git origin)
/// FAILS CLOSED with `wrong_repository`: neither side is trusted once
/// they disagree. An unanchored checkout (no slug anywhere) FAILS
/// CLOSED with `repository_unanchored`: a digest-shaped repository ID
/// is not proof it is this repository, and no shape-only fallback can
/// bless it. The old warning-suffices path is deleted, not deprecated.
fn validate_repository(
    manifest: &BaselineManifest,
    expected: &ProvenanceExpectations,
) -> Result<(), String> {
    reject(!expected.repository_conflict, "wrong_repository")?;
    let Some(anchored) = &expected.repository_id else {
        return Err("repository_unanchored".to_owned());
    };
    reject(manifest.repository_id == *anchored, "wrong_repository")?;
    Ok(())
}

/// Workflow-ref check: `<owner>/<repo>/<workflow>@<protected ref>`.
///
/// The path must equal the generated workflow and the ref must equal
/// both the protected branch ref and the manifest's own ref;
/// direct-execution proof from any other workflow or ref never becomes
/// default-branch evidence. The slug must name the anchored repository
/// and agree with the manifest's own repository id, so an evil-fork
/// slug can neither match the anchor nor the manifest binding.
fn validate_workflow_ref(
    manifest: &BaselineManifest,
    expected: &ProvenanceExpectations,
) -> Result<(), String> {
    let Some((slug, path, git_ref)) = parse_workflow_ref(&manifest.workflow_ref) else {
        return Err("bad_workflow_ref".to_owned());
    };
    let protected = format!("refs/heads/{}", expected.branch);
    let exact = !slug.is_empty() && path == expected.workflow_path && git_ref == protected;
    reject(exact, "wrong_workflow")?;
    reject(git_ref == manifest.ref_, "wrong_workflow")?;
    let slug = slug.to_lowercase();
    let Some(anchored) = expected.repository_slug.as_deref() else {
        return Err("repository_unanchored".to_owned());
    };
    reject(slug == anchored, "wrong_repository")?;
    reject(
        digest_b3(format!("github.com/{slug}").as_bytes()) == manifest.repository_id,
        "wrong_repository",
    )?;
    Ok(())
}

/// Split `<owner>/<repo>/<workflow path>@<ref>` into its three parts.
///
/// Splits at the first `@`: neither the repo slug nor the generated
/// workflow path contains one, while branch names may.
pub(crate) fn parse_workflow_ref(input: &str) -> Option<(String, String, String)> {
    let (left, git_ref) = input.split_once('@')?;
    if git_ref.is_empty() || !git_ref.starts_with("refs/") {
        return None;
    }
    let mut segments = left.split('/');
    let owner = segments.next()?;
    let repo = segments.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let path: Vec<&str> = segments.collect();
    if path.is_empty() || path.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some((
        format!("{owner}/{repo}"),
        path.join("/"),
        git_ref.to_owned(),
    ))
}

/// Lowercase `owner/repo` slug from the git origin URL, when the origin
/// is a `github.com` remote a workflow slug can name.
///
/// Resolution runs through the shared [`velnor_actions_orchestrator_core::origin::origin_url_via_git`]
/// helper, so linked worktrees, includes, and worktree configuration all
/// follow Git semantics. Other hosts have no slug form comparable to
/// `owner/repo`; those checkouts fail closed in [`validate_repository`],
/// never warn-and-proceed.
pub(crate) fn repository_slug_from_origin(root: &std::path::Path) -> Option<String> {
    let url = velnor_actions_orchestrator_core::origin::origin_url_via_git(root)?;
    normalize_origin_url(&url).and_then(|normalized| {
        normalized
            .strip_prefix("github.com/")
            .map(str::to_owned)
            .filter(|slug| {
                let mut parts = slug.split('/');
                matches!(
                    (parts.next(), parts.next(), parts.next()),
                    (Some(owner), Some(repo), None)
                        if !owner.is_empty() && !repo.is_empty()
                )
            })
    })
}

/// Normalize an origin URL to `host/path` for identity comparison.
///
/// Accepts `https://` (and `http://`) plus scp-like `user@host:path`
/// forms; strips credentials, ports, and trailing `.git`; hosting
/// slugs compare case-insensitively.
fn normalize_origin_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if trimmed.is_empty() {
        return None;
    }
    let (hostport, path) = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://"))
    {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
        rest.split_once('/')?
    } else {
        let rest = trimmed.rsplit_once('@').map_or(trimmed, |(_, rest)| rest);
        rest.split_once(':')?
    };
    let host = hostport.split_once(':').map_or(hostport, |(host, _)| host);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!(
        "{}/{}",
        host.to_lowercase(),
        path.trim_matches('/').to_lowercase()
    ))
}
