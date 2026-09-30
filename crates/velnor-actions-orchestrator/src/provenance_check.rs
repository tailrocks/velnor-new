//! Validated baseline provenance: expected values, never bare syntax.
//!
//! Declared via `#[path]` from `cover_baseline.rs` (no `lib.rs` edit).
//! A [`ValidatedProvenance`] binds the actual repository, protected
//! workflow/ref/event, exact base commit, successful run/attempt,
//! artifact name plus service ID, manifest digest, schema version, and
//! generator plus task compatibility.

use velnor_actions_contract::{digest_b3, validate_digest};

use crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA;
use crate::merge::BaselineManifest;

// Unit tests live here so `provenance_check.rs` keeps its size gate.
#[cfg(test)]
#[path = "provenance_check_tests.rs"]
mod provenance_check_tests;

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
}

/// Provenance validated against [`ProvenanceExpectations`].
///
/// Only validated values construct coverage proofs and baseline records;
/// validated-but-uncarried dimensions (event, ref, workflow, attempt,
/// schema, generator, repository) are enforced by validation.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedProvenance {
    /// Exact trusted source commit.
    pub(crate) source_commit: String,
    /// Proof run ID.
    pub(crate) run_id: u64,
    /// Derived baseline artifact name.
    pub(crate) artifact_name: String,
    /// Numeric baseline artifact ID.
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
    let sha = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
    reject(sha, "bad_source_commit")?;
    validate_digest(compat).map_err(|_| "bad_compatibility_id".to_owned())?;
    Ok(format!("velnor-baseline-{commit}-{compat}"))
}

/// True only for protected pushes; PR/fork/merge-group runs never publish.
pub(crate) fn publish_event_eligible(event: velnor_actions_contract::WorkflowEvent) -> bool {
    event == velnor_actions_contract::WorkflowEvent::Push
}

/// Reject a failed evidence check with its reason.
fn reject(ok: bool, reason: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(reason.to_owned()) }
}

/// Carried proof binds the task entry's identity and originating run.
///
/// `ManifestTaskProof` exposes no getters, so the comparison runs over its
/// canonical serialization; a serialization failure never validates.
fn proof_matches_task(
    proof: &velnor_actions_contract::ManifestTaskProof,
    task: &crate::merge::required_evidence::BaselineTaskEntry,
) -> bool {
    let Ok(value) = serde_json::to_value(proof) else {
        return false;
    };
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    field("task_id") == Some(task.task_id.as_str())
        && field("task_digest") == Some(task.task_digest.as_str())
        && field("input_digest") == Some(task.input_digest.as_str())
        && value
            .get("proof_run_id")
            .and_then(serde_json::Value::as_u64)
            == Some(task.proof_run_id)
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
    if crate::internal_plan::snapshot::check_canonical_version(manifest.schema).is_err() {
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
    for task in &manifest.tasks {
        if let Some(proof) = &task.proof {
            proof.validate().map_err(|_| "bad_task_proof".to_owned())?;
            reject(proof_matches_task(proof, task), "proof_mismatch")?;
            continue;
        }
        let ids_ok = velnor_actions_contract::validate_task_id(&task.task_id).is_ok()
            && validate_digest(&task.task_digest).is_ok()
            && validate_digest(&task.input_digest).is_ok();
        let runs_ok = task.proof_run_id > 0 && task.observed_run_id > 0;
        let fresh_ok = task
            .external_data
            .as_ref()
            .is_none_or(|proof| proof.validate().is_ok());
        reject(ids_ok, "bad_task_identity")?;
        reject(runs_ok, "bad_proof_identity")?;
        reject(fresh_ok, "bad_external_data")?;
    }
    Ok(ValidatedProvenance {
        source_commit: manifest.source_commit.clone(),
        run_id: manifest.run_id,
        artifact_name: manifest.artifact_name.clone(),
        artifact_id: manifest.artifact_id,
        manifest_digest: manifest_digest.to_owned(),
    })
}

/// True for generator SHAs no manifest may bind: empty, all-zero, or the
/// explicit unresolved marker.
fn is_unverifiable_generator_sha(sha: &str) -> bool {
    sha.is_empty()
        || sha == UNRESOLVED_GENERATOR_SHA
        || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// Repository check: exact match against the git-origin anchor.
///
/// An unanchored checkout (no git origin) FAILS CLOSED with
/// `repository_unanchored`: a digest-shaped repository ID is not proof
/// it is this repository, and no shape-only fallback can bless it.
/// The old warning-suffices path is deleted, not deprecated.
fn validate_repository(
    manifest: &BaselineManifest,
    expected: &ProvenanceExpectations,
) -> Result<(), String> {
    let Some(anchored) = &expected.repository_id else {
        return Err("repository_unanchored".to_owned());
    };
    reject(manifest.repository_id == *anchored, "wrong_repository")?;
    Ok(())
}

/// Workflow-ref check: `<owner>/<repo>/<workflow>@<protected ref>`.
///
/// The path must equal the generated workflow and the ref must equal
/// the protected branch ref; direct-execution proof from any other
/// workflow or ref never becomes default-branch evidence.
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
    Ok(())
}

/// Split `<owner>/<repo>/<workflow path>@<ref>` into its three parts.
///
/// Splits at the first `@`: neither the repo slug nor the generated
/// workflow path contains one, while branch names may.
fn parse_workflow_ref(input: &str) -> Option<(String, String, String)> {
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

/// Repository identity digest from the git origin URL, when configured.
///
/// Returns `None` when no origin remote exists; unanchored checkouts
/// fail closed in [`validate_repository`], never warn-and-proceed.
pub(crate) fn repository_anchor_from_origin(root: &std::path::Path) -> Option<String> {
    let config = std::fs::read_to_string(root.join(".git/config")).ok()?;
    let url = origin_url(&config)?;
    normalize_origin_url(&url).map(|normalized| digest_b3(normalized.as_bytes()))
}

/// Origin URL from `.git/config` text, if an origin remote exists.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
            continue;
        }
        if !in_origin {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            let value = value.trim().trim_matches('"').trim().to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
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
