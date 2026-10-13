//! Exact-base baseline lookup and manifest resolution.
//!
//! Finds the exact-base successful run through pinned `gh`, downloads the
//! single exact named artifact, and keeps temp entries matching the exact
//! base, run, attempt, id, and shape.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::merge::BaselineManifest;
use crate::run_select::{select_baseline_artifact, select_exact_base_run};

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
        let Some(repo) = velnor_actions_orchestrator_core::origin::validate_repository_slug(repo)
        else {
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
        for value in [workflow, branch] {
            let spam = ["://", "*", "$", ";", " "]
                .iter()
                .any(|t| value.contains(t));
            if value.trim().is_empty() || spam {
                return Err("bad_lookup_input".into());
            }
        }
        Ok(())
    }

    /// Fixed `gh run list` args for the exact workflow and branch.
    ///
    /// `--repo` pins the lookup to the expected repository: without it
    /// `gh` would resolve the repo from the mutable git origin. The
    /// listing carries the successful attempt: selection binds the
    /// baseline claim to this attempt, never an unpinned run.
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

    /// Fixed `gh api` args listing one run's artifacts.
    ///
    /// The path names the expected repository explicitly: no
    /// `{owner}`/`{repo}` template ever resolves from the mutable git
    /// origin. The response proves the exact baseline artifact exists
    /// unexpired before any download is attempted.
    #[must_use]
    pub(crate) fn artifacts_args(&self, run_id: u64) -> Vec<OsString> {
        [
            "api",
            &format!("repos/{}/actions/runs/{run_id}/artifacts", self.repo),
        ]
        .iter()
        .map(OsString::from)
        .collect()
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
/// Only one exact named artifact ever downloads: without a known
/// artifact name there is no bounded download, so the lookup misses
/// before spawning anything. A whole-run download fallback does not
/// exist. Every `gh` call carries the expected repository explicitly;
/// a conflicted or unresolvable repo misses before spawning anything.
/// The selected run pins its successful attempt and the exact
/// artifact must exist unexpired before the download; survivors must
/// match the exact base commit, run, attempt, name fingerprint, and
/// directory name; temp is always removed.
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
    let repo = resolve_lookup_repo(root, repository)?;
    let lookup = BaselineLookup::new(base, workflow, branch, &repo)?;
    let text = BaselineLookup::run(catalog, root, lookup.list_args())?;
    let selected = select_exact_base_run(&text, base, branch)?;
    let listed = BaselineLookup::run(catalog, root, lookup.artifacts_args(selected.run_id))?;
    // Existence plus freshness only: the listing proves the exact
    // artifact is present unexpired in the exact run. Its numeric
    // service ID never enters the manifest binding: the manifest
    // carries the name fingerprint, which the publisher can derive
    // before uploading and the service ID can never satisfy.
    select_baseline_artifact(&listed, artifact)?;
    let temp = tempfile::tempdir().map_err(|_| "baseline_unavailable".to_owned())?;
    BaselineLookup::run(
        catalog,
        root,
        crate::cover_baseline::baseline_download_args(
            base,
            workflow,
            branch,
            Some(artifact),
            selected.run_id,
            temp.path(),
            &repo,
        ),
    )?;
    collect_manifests(
        temp.path(),
        base,
        selected.run_id,
        selected.attempt,
        artifact,
    )
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
mod tests;
