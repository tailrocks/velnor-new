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
}

impl BaselineLookup {
    /// Build a lookup; rejects short SHAs, URLs, wildcards, and shell.
    /// # Errors
    pub(crate) fn new(base: &str, workflow: &str, branch: &str) -> Result<Self, String> {
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
        Ok(Self {
            base_sha: base.into(),
            workflow: workflow.into(),
            branch: branch.into(),
        })
    }

    /// Fixed `gh run list` args for the exact workflow and branch.
    ///
    /// The listing carries the successful attempt: selection binds the
    /// baseline claim to this attempt, never an unpinned run.
    #[must_use]
    pub(crate) fn list_args(&self) -> Vec<OsString> {
        let fields = "databaseId,headSha,event,conclusion,headBranch,attempt";
        let workflow = self.workflow.as_str();
        let branch = self.branch.as_str();
        [
            "run",
            "list",
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
    /// `{owner}`/`{repo}` fill from the current repository; the response
    /// proves the exact baseline artifact exists unexpired before any
    /// download is attempted.
    #[must_use]
    pub(crate) fn artifacts_args(run_id: u64) -> Vec<OsString> {
        [
            "api",
            &format!("repos/{{owner}}/{{repo}}/actions/runs/{run_id}/artifacts"),
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
        output
            .stdout_text("gh")
            .map_err(|_| "baseline_unavailable".to_owned())
    }
}

/// Resolve exact-base manifests: list, select, pin, download, filter.
///
/// Only one exact named artifact ever downloads: without a known
/// artifact name there is no bounded download, so the lookup misses
/// before spawning anything. A whole-run download fallback does not
/// exist. The selected run pins its successful attempt and the exact
/// artifact must exist unexpired before the download; survivors must
/// match the exact base commit, run, attempt, artifact id, and
/// directory name; temp is always removed.
/// # Errors
pub(crate) fn resolve_manifests(
    catalog: &ToolCatalog,
    root: &Path,
    base: &str,
    workflow: &str,
    branch: &str,
    artifact: Option<&str>,
) -> Result<Vec<BaselineManifest>, String> {
    let lookup = BaselineLookup::new(base, workflow, branch)?;
    let Some(artifact) = artifact.filter(|name| !name.is_empty()) else {
        return Err("baseline_no_exact_artifact".to_owned());
    };
    let text = BaselineLookup::run(catalog, root, lookup.list_args())?;
    let selected = select_exact_base_run(&text, base, branch)?;
    let listed = BaselineLookup::run(
        catalog,
        root,
        BaselineLookup::artifacts_args(selected.run_id),
    )?;
    let artifact_id = select_baseline_artifact(&listed, artifact)?;
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
        ),
    )?;
    collect_manifests(
        temp.path(),
        base,
        selected.run_id,
        selected.attempt,
        artifact_id,
    )
}

/// Keep temp artifacts matching the exact base, run, attempt, id, shape.
fn collect_manifests(
    dir: &Path,
    base: &str,
    run_id: u64,
    attempt: u64,
    artifact_id: u64,
) -> Result<Vec<BaselineManifest>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|_| "baseline_unavailable".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "baseline_unavailable".to_owned())?;
        if let Some(manifest) = crate::cover_baseline::baseline_entry_for(
            &entry.path(),
            base,
            run_id,
            attempt,
            artifact_id,
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
