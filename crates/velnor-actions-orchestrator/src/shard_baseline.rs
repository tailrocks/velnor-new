//! Exact-base baseline lookup and manifest resolution.
//!
//! Selects one successful exact-base run through pinned `gh`, then authenticates
//! its exact-name artifact by service ID/digest and its manifest's API attempt.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use velnor_actions_mise::{PinnedTool, PinnedToolExec, RuntimePaths, ToolCatalog};

use crate::run_select::{select_baseline_artifacts, select_exact_base_run};

#[path = "baseline_artifact_transport.rs"]
pub(crate) mod artifact_transport;
pub(crate) use artifact_transport::AcquiredBaseline;

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
    /// successful summary supplies the current attempt as an upper bound;
    /// each publication attempt is authenticated through its exact API.
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
    /// origin. The selector checks pagination completeness and artifact
    /// identity, including expiry, before any download is attempted.
    #[must_use]
    pub(crate) fn artifacts_args(&self, run_id: u64) -> Vec<OsString> {
        [
            "api",
            &format!("repos/{}/actions/runs/{run_id}/artifacts", self.repo),
            "--paginate",
            "--slurp",
        ]
        .iter()
        .map(OsString::from)
        .collect()
    }

    /// Fixed API path for one original workflow attempt.
    #[must_use]
    pub(crate) fn attempt_args(&self, run_id: u64, attempt: u64) -> Vec<OsString> {
        [
            "api",
            &format!(
                "repos/{}/actions/runs/{run_id}/attempts/{attempt}",
                self.repo
            ),
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
        Self::run_in_runtime(catalog, root, args, RuntimePaths::full())
    }

    /// Run fixed `gh` args under an explicit compiled runtime domain.
    pub(crate) fn run_in_runtime(
        catalog: &ToolCatalog,
        root: &Path,
        args: Vec<OsString>,
        runtime: RuntimePaths,
    ) -> Result<String, String> {
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args);
        let exec = exec.map_err(|err| err.to_string())?;
        let output = exec
            .command_with_runtime(catalog, runtime)
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
/// One megabyte fails closed before JSON parsing; the artifact selector
/// separately requires every page and entry within its count bound. (The
/// spawn layer caps pipes at 8 MiB; this is the tighter call-site bound.)
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

/// Resolve one exact-base baseline from a bounded run summary and a complete
/// artifact listing.
///
/// The artifact name derives only from base and compatibility. The successful
/// run summary selects the run and bounds the allowed manifest attempt.
/// Service metadata binds the run; the exact attempt API response binds the
/// manifest attempt. Every `gh` call carries the expected repository
/// explicitly; unavailable evidence misses to full work.
/// # Errors
pub(crate) fn resolve_manifests(
    catalog: &ToolCatalog,
    root: &Path,
    base: &str,
    workflow: &str,
    branch: &str,
    artifact_name: &str,
    repository: Option<&str>,
    runtime: RuntimePaths,
) -> Result<AcquiredBaseline, String> {
    BaselineLookup::validate_inputs(base, workflow, branch)?;
    let prefix = format!("velnor-baseline-{base}-");
    let compatibility = artifact_name
        .strip_prefix(&prefix)
        .ok_or_else(|| "baseline_no_exact_artifact".to_owned())?;
    let canonical = velnor_actions_contract::artifact_id_for_baseline(base, compatibility)
        .map_err(|_| "baseline_no_exact_artifact".to_owned())?;
    if canonical != artifact_name {
        return Err("baseline_no_exact_artifact".to_owned());
    }
    let repo = resolve_lookup_repo(root, repository)?;
    let lookup = BaselineLookup::new(base, workflow, branch, &repo)?;
    resolve_with(
        &lookup,
        artifact_name,
        compatibility,
        |args| BaselineLookup::run_in_runtime(catalog, root, args, runtime),
        |receipt| artifact_transport::download_archive(catalog, root, &lookup, receipt, runtime),
    )
}

/// One transport-injected normal lookup, retaining authenticated receipts.
fn resolve_with(
    lookup: &BaselineLookup,
    expected: &str,
    compatibility: &str,
    mut run: impl FnMut(Vec<OsString>) -> Result<String, String>,
    mut download: impl FnMut(&crate::run_select::BaselineArtifactReceipt) -> Result<Vec<u8>, String>,
) -> Result<AcquiredBaseline, String> {
    let text = run(lookup.list_args())?;
    let selected = select_exact_base_run(&text, &lookup.base_sha, &lookup.branch)?;
    let listed = run(lookup.artifacts_args(selected.run_id))?;
    let receipts = select_baseline_artifacts(
        &listed,
        expected,
        &lookup.base_sha,
        compatibility,
        &lookup.branch,
        selected.run_id,
    )?;
    if receipts.is_empty() {
        return Err("baseline_not_found".to_owned());
    }
    let mut verified_matches = Vec::new();
    for receipt in receipts {
        let Ok(bytes) = download(&receipt) else {
            continue;
        };
        let Ok(verified) = artifact_transport::verify_archive(receipt, &bytes) else {
            continue;
        };
        let attempt = verified.manifest().run_attempt;
        if attempt > selected.attempt {
            continue;
        }
        let Ok(record) = run(lookup.attempt_args(selected.run_id, attempt)) else {
            continue;
        };
        let Ok(acquired) = artifact_transport::authenticate_attempt(verified, &record, lookup)
        else {
            continue;
        };
        verified_matches.push(acquired);
    }
    let newest_attempt = verified_matches
        .iter()
        .map(|acquired| acquired.attempt_receipt().run_attempt)
        .max();
    let Some(newest_attempt) = newest_attempt else {
        return Err("baseline_unavailable".to_owned());
    };
    let mut newest_matches = verified_matches
        .into_iter()
        .filter(|acquired| acquired.attempt_receipt().run_attempt == newest_attempt);
    let Some(newest) = newest_matches.next() else {
        return Err("baseline_unavailable".to_owned());
    };
    if newest_matches.next().is_some() {
        return Err("baseline_artifact_ambiguous".to_owned());
    }
    Ok(newest)
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

#[cfg(test)]
#[path = "shard_baseline_retry_tests.rs"]
mod retry_tests;
#[cfg(test)]
#[path = "shard_baseline_tests.rs"]
mod tests;
