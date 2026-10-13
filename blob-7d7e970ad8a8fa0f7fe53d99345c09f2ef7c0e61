//! Exact-base baseline lookup and manifest resolution.
//!
//! Finds exact-base candidates through pinned `gh`, fetches the exact
//! service artifact ZIP by ID, and keeps temp entries matching the base,
//! run, original successful attempt, manifest ID, and shape.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::merge::BaselineManifest;
use crate::run_select::{BaselineArtifactMetadata, select_exact_base_candidates};

#[path = "baseline_archive.rs"]
pub(crate) mod archive;

/// Exact-base baseline lookup through pinned `gh` (par §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaselineLookup {
    /// Full 40-hex base commit SHA.
    pub(crate) base_sha: String,
    /// Generated workflow path.
    pub(crate) workflow: String,
    /// Protected default-branch name.
    pub(crate) branch: String,
    /// Expected repository slug scoping every `gh` call.
    pub(crate) repo: String,
}

impl BaselineLookup {
    /// Build a lookup; rejects short SHAs, URLs, wildcards, shell, and
    /// malformed repo slugs.
    /// # Errors
    pub(crate) fn new(
        base: &str,
        workflow: &str,
        branch: &str,
        repo: &str,
    ) -> Result<Self, String> {
        Self::validate_inputs(base, workflow, branch)?;
        let Some(repo) = crate::origin::validate_repository_slug(repo) else {
            return Err("bad_lookup_repo".into());
        };
        Ok(Self {
            base_sha: base.into(),
            workflow: workflow.into(),
            branch: branch.into(),
            repo,
        })
    }

    /// Validate base, workflow, and branch shapes without a repo.
    ///
    /// [`resolve_manifests`] runs this first so malformed inputs fail
    /// deterministically before any environment-dependent repo miss.
    /// # Errors
    pub(crate) fn validate_inputs(base: &str, workflow: &str, branch: &str) -> Result<(), String> {
        if base.len() != 40 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("base_must_be_full_sha".into());
        }
        if !velnor_actions_contract::is_valid_branch_name(branch) {
            return Err("bad_lookup_input".into());
        }
        let spam = ["://", "*", "$", ";", " "]
            .iter()
            .any(|token| workflow.contains(token));
        if workflow.trim().is_empty() || spam {
            return Err("bad_lookup_input".into());
        }
        Ok(())
    }

    /// Fixed `gh run list` args for the exact workflow and branch.
    ///
    /// `--repo` pins the lookup to the expected repository: without it
    /// `gh` would resolve the repo from the mutable git origin. The
    /// listing carries the current attempt only for candidate discovery;
    /// the original proof attempt is authenticated through its exact API.
    #[must_use]
    pub(crate) fn list_args(&self) -> Vec<OsString> {
        let fields = "databaseId,headSha,event,conclusion,headBranch,attempt";
        let workflow = self.workflow.as_str();
        let branch = self.branch.as_str();
        [
            "run",
            "list",
            "--repo",
            self.repo.as_str(),
            "--workflow",
            workflow,
            "--branch",
            branch,
            "--json",
            fields,
            "--limit",
            "50",
        ]
        .iter()
        .map(OsString::from)
        .collect()
    }

    /// Fixed exact service-artifact ZIP endpoint for one positive ID.
    /// # Errors
    /// Rejects zero, which cannot identify a service artifact.
    pub(crate) fn artifact_zip_args(&self, artifact_id: u64) -> Result<Vec<OsString>, String> {
        if artifact_id == 0 {
            return Err("baseline_unavailable".to_owned());
        }
        Ok(vec![
            OsString::from("api"),
            OsString::from(format!(
                "repos/{}/actions/artifacts/{artifact_id}/zip",
                self.repo
            )),
        ])
    }

    /// Run fixed `gh` args under the pinned catalog in `root`.
    pub(crate) fn run(
        catalog: &ToolCatalog,
        root: &Path,
        args: Vec<OsString>,
    ) -> Result<String, String> {
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args);
        let exec = exec.map_err(|err| err.to_string())?;
        let output = exec
            .command(catalog)
            .map_err(|err| err.to_string())?
            .with_cwd(PathBuf::from(root))
            .run()
            .map_err(|_| "baseline_unavailable".to_owned())?;
        if !output.success {
            return Err("baseline_unavailable".to_owned());
        }
        gh_stdout_checked(&output)
    }

    /// Run fixed `gh api` args whose successful response is binary ZIP data.
    ///
    /// # Errors
    /// Returns a baseline miss for any command or service failure.
    pub(crate) fn run_archive(
        catalog: &ToolCatalog,
        root: &Path,
        args: Vec<OsString>,
    ) -> Result<Vec<u8>, String> {
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args);
        let exec = exec.map_err(|_| "baseline_unavailable".to_owned())?;
        let output = exec
            .command(catalog)
            .map_err(|_| "baseline_unavailable".to_owned())?
            .with_cwd(PathBuf::from(root))
            .run_bounded(
                archive::MAX_BASELINE_ARCHIVE_BYTES,
                Duration::from_secs(velnor_actions_mise::command::RUN_TIMEOUT_SECS),
            )
            .map_err(|_| "baseline_unavailable".to_owned())?;
        if output.success {
            Ok(output.stdout)
        } else {
            Err("baseline_unavailable".to_owned())
        }
    }
}

/// Maximum accepted `gh` stdout bytes for lookup and retrieve calls.
///
/// Listings and download receipts are kilobytes; one megabyte fails
/// closed on runaway output before JSON parsing instead of buffering
/// megabytes the selectors never need. (The spawn layer caps pipes at
/// 8 MiB; this is the tighter call-site bound.)
const MAX_GH_STDOUT_BYTES: usize = 1 << 20;

/// Decode `gh` stdout with an explicit size cap.
///
/// Over-bound output misses before parsing; undecodable output misses
/// the same way.
fn gh_stdout_checked(output: &velnor_actions_mise::ProcessOutput) -> Result<String, String> {
    if output.stdout.len() > MAX_GH_STDOUT_BYTES {
        return Err("baseline_unavailable".to_owned());
    }
    output
        .stdout_text("gh")
        .map_err(|_| "baseline_unavailable".to_owned())
}

/// Resolve exact-base manifests: list, select, pin, download, filter.
///
/// Only one exact artifact ID ever downloads: without a known artifact
/// name there is no bounded fetch, so lookup misses before spawning.
/// A whole-run fetch fallback does not exist. Every `gh` call carries
/// the expected repository explicitly;
/// a conflicted or unresolvable repo misses before spawning anything.
/// The exact artifact must exist unexpired before download; its original
/// successful attempt is authenticated independently from live service
/// metadata. A later retry never relabels the immutable artifact's proof.
/// # Errors
pub(crate) fn resolve_manifests(
    catalog: &ToolCatalog,
    root: &Path,
    base: &str,
    workflow: &str,
    branch: &str,
    artifact: Option<&str>,
    repository: Option<&str>,
) -> Result<Vec<BaselineManifest>, String> {
    BaselineLookup::validate_inputs(base, workflow, branch)?;
    let Some(artifact) = artifact.filter(|name| !name.is_empty()) else {
        return Err("baseline_no_exact_artifact".to_owned());
    };
    velnor_actions_contract::validate_artifact_id(artifact)
        .map_err(|_| "baseline_no_exact_artifact".to_owned())?;
    let repo = resolve_lookup_repo(root, repository)?;
    let lookup = BaselineLookup::new(base, workflow, branch, &repo)?;
    resolve_with(
        &lookup,
        artifact,
        |args| BaselineLookup::run(catalog, root, args),
        |args| BaselineLookup::run_archive(catalog, root, args),
    )
}

/// One transport-injected normal lookup, retaining original publication proof.
fn resolve_with(
    lookup: &BaselineLookup,
    artifact: &str,
    mut run: impl FnMut(Vec<OsString>) -> Result<String, String>,
    mut run_archive: impl FnMut(Vec<OsString>) -> Result<Vec<u8>, String>,
) -> Result<Vec<BaselineManifest>, String> {
    let text = run(lookup.list_args())?;
    for run_id in select_exact_base_candidates(&text, &lookup.base_sha, &lookup.branch)? {
        let Ok(metadata) = crate::baseline_artifact_listing::select_metadata(
            &lookup.repo,
            run_id,
            artifact,
            &mut run,
        ) else {
            continue;
        };
        if let Ok(parent) = download_authenticated(
            lookup,
            artifact,
            &metadata,
            run_id,
            &mut run,
            &mut run_archive,
        ) {
            return Ok(vec![parent]);
        }
    }
    Err("baseline_unavailable".to_owned())
}

/// Read an exact artifact and authenticate its original successful attempt.
fn download_authenticated(
    lookup: &BaselineLookup,
    artifact: &str,
    metadata: &BaselineArtifactMetadata,
    run_id: u64,
    run: &mut impl FnMut(Vec<OsString>) -> Result<String, String>,
    run_archive: &mut impl FnMut(Vec<OsString>) -> Result<Vec<u8>, String>,
) -> Result<BaselineManifest, String> {
    let temp = tempfile::tempdir().map_err(|_| "baseline_unavailable".to_owned())?;
    let archive_bytes = run_archive(lookup.artifact_zip_args(metadata.id)?)?;
    archive::stage_baseline_archive(temp.path(), artifact, metadata, &archive_bytes)?;
    let path = temp.path().join(artifact).join("baseline.json");
    let text = crate::retrieve_reports::read_staged_text(
        &path,
        crate::retrieve_reports::MAX_STAGED_REPORT_BYTES,
    )
    .map_err(|_| "baseline_unavailable".to_owned())?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(&text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let candidate: BaselineManifest =
        serde_json::from_value(value).map_err(|_| "baseline_unavailable".to_owned())?;
    let mut found = collect_manifests(
        temp.path(),
        &lookup.base_sha,
        run_id,
        candidate.run_attempt,
        artifact,
    )?;
    let parent = found
        .pop()
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let endpoint = format!(
        "repos/{}/actions/runs/{run_id}/attempts/{}",
        lookup.repo, parent.run_attempt
    );
    let record = run(vec![OsString::from("api"), OsString::from(endpoint)])?;
    if !crate::retrieve_baseline::authentic_attempt(
        &record,
        &parent,
        &lookup.repo,
        &lookup.branch,
        &lookup.workflow,
    ) {
        return Err("baseline_unauthenticated".to_owned());
    }
    Ok(parent)
}

/// Repository slug scoping every lookup `gh` call.
///
/// Request-first with origin fallback; a conflict or an absence misses
/// before spawning anything, so no lookup ever queries a repo the
/// runner did not bless or the checkout cannot name.
fn resolve_lookup_repo(root: &Path, repository: Option<&str>) -> Result<String, String> {
    let origin = crate::cover_baseline::provenance_check::repository_slug_from_origin(root);
    let expected = crate::cover_baseline::provenance_resolve::resolve_expected_repository(
        origin.as_deref(),
        repository,
    );
    if expected.conflict {
        return Err("baseline_repo_conflict".to_owned());
    }
    expected
        .slug
        .ok_or_else(|| "baseline_repo_unresolved".to_owned())
}

/// Keep temp artifacts matching the exact base, run, attempt, id, shape.
///
/// The expected numeric ID derives from the artifact name, matching
/// the publisher-written fingerprint; the service listing ID never
/// binds manifest bytes.
fn collect_manifests(
    dir: &Path,
    base: &str,
    run_id: u64,
    attempt: u64,
    artifact: &str,
) -> Result<Vec<BaselineManifest>, String> {
    let expected = crate::cover_compat::baseline_artifact_numeric_id(artifact);
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|_| "baseline_unavailable".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "baseline_unavailable".to_owned())?;
        if let Some(manifest) = crate::cover_baseline::baseline_entry_for(
            &entry.path(),
            base,
            run_id,
            attempt,
            expected,
        ) {
            out.push(manifest);
        }
    }
    if out.is_empty() {
        return Err("baseline_unavailable".to_owned());
    }
    out.sort_by(|left, right| left.artifact_name.cmp(&right.artifact_name));
    Ok(out)
}

#[cfg(test)]
#[path = "shard_baseline_retry_tests.rs"]
mod retry_tests;
#[cfg(test)]
#[path = "shard_baseline_tests.rs"]
mod tests;
