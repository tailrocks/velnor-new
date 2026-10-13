//! Exact-base baseline lookup through pinned `gh` (par §5).
//!
//! Fixed `run list` filter args plus exact run-and-artifact download
//! args. Inputs reject short SHAs, URLs, wildcards, and shell text;
//! the artifact name is exact, never a pattern, and the numeric run id
//! is typed `u64` so no id, URL, or shell fragment reaches the vector.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::requests::PinnedToolExec;
use velnor_actions_mise_core::command::IsolatedCommand;
use velnor_actions_mise_core::error::MiseError;

/// Full commit SHA length required for the exact base.
const FULL_SHA_LEN: usize = 40;

/// Fields selecting identity plus headSha, event, and conclusion.
const LIST_FIELDS: &str = "databaseId,headSha,event,conclusion,headBranch";

/// `run list` result cap.
const LIST_LIMIT: &str = "50";

/// Payload program for every lookup invocation.
const GH_PROGRAM: &str = "gh";

/// Exact-base baseline lookup with a fixed artifact name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineLookup {
    /// Full 40-hex base commit SHA.
    base_sha: String,
    /// Generated workflow path.
    workflow: String,
    /// Protected default-branch name.
    branch: String,
    /// Exact coverage-manifest artifact name.
    artifact: String,
}

impl BaselineLookup {
    /// Build a lookup; rejects short SHAs, URLs, wildcards, and shell.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidBaselineInput`] for a short or
    /// non-hex base, a blank or hostile workflow/branch, or a blank or
    /// non-token artifact name.
    pub fn new(
        base: &str,
        workflow: &str,
        branch: &str,
        artifact: &str,
    ) -> Result<Self, MiseError> {
        if base.len() != FULL_SHA_LEN || !base.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid_input("base_sha", base));
        }
        check_lookup_text("workflow", workflow)?;
        check_lookup_text("branch", branch)?;
        check_artifact(artifact)?;
        Ok(Self {
            base_sha: base.to_owned(),
            workflow: workflow.to_owned(),
            branch: branch.to_owned(),
            artifact: artifact.to_owned(),
        })
    }

    /// Full 40-hex base commit SHA.
    #[must_use]
    pub fn base_sha(&self) -> &str {
        &self.base_sha
    }

    /// Generated workflow path.
    #[must_use]
    pub fn workflow(&self) -> &str {
        &self.workflow
    }

    /// Protected default-branch name.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Exact coverage-manifest artifact name.
    #[must_use]
    pub fn artifact(&self) -> &str {
        &self.artifact
    }

    /// Fixed `gh run list` args for the exact workflow and branch.
    #[must_use]
    pub fn list_args(&self) -> Vec<OsString> {
        [
            "run",
            "list",
            "--workflow",
            self.workflow.as_str(),
            "--branch",
            self.branch.as_str(),
            "--json",
            LIST_FIELDS,
            "--limit",
            LIST_LIMIT,
        ]
        .iter()
        .map(OsString::from)
        .collect()
    }

    /// Fixed `gh run download` args for one exact run and artifact.
    #[must_use]
    pub fn download_args(&self, run_id: u64, dir: &Path) -> Vec<OsString> {
        [
            OsString::from("run"),
            OsString::from("download"),
            OsString::from(run_id.to_string()),
            OsString::from("--name"),
            OsString::from(&self.artifact),
            OsString::from("--dir"),
            dir.as_os_str().to_owned(),
        ]
        .into_iter()
        .collect()
    }

    /// Full mise argv running the list under pinned `gh`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::ForbiddenPayload`] only if the fixed payload
    /// were forbidden, which construction rules out.
    pub fn list_argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        Ok(Self::exec_for(self.list_args())?.argv(catalog))
    }

    /// Full mise argv downloading one exact run plus artifact.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::ForbiddenPayload`] only if the fixed payload
    /// were forbidden, which construction rules out.
    pub fn download_argv(
        &self,
        catalog: &ToolCatalog,
        run_id: u64,
        dir: &Path,
    ) -> Result<Vec<OsString>, MiseError> {
        Ok(Self::exec_for(self.download_args(run_id, dir))?.argv(catalog))
    }

    /// Isolated command running fixed `gh` args under the pinned catalog.
    ///
    /// The `gh` tool selects [`EnvPolicy::Baseline`](velnor_actions_mise_core::command::EnvPolicy):
    /// baseline lookup keeps the ambient CI identity for API reads while
    /// every other credential is stripped before spawn.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] for empty args and
    /// [`MiseError::ForbiddenPayload`] only if the fixed payload were
    /// forbidden, which construction rules out.
    pub fn command(
        &self,
        catalog: &ToolCatalog,
        args: Vec<OsString>,
    ) -> Result<IsolatedCommand, MiseError> {
        Self::exec_for(args)?.command(catalog)
    }

    /// Pinned `gh` execution for validated fixed args.
    fn exec_for(args: Vec<OsString>) -> Result<PinnedToolExec, MiseError> {
        PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new(GH_PROGRAM), args)
    }
}

/// Reject blank values and URL, wildcard, variable, or shell text.
fn check_lookup_text(field: &str, value: &str) -> Result<(), MiseError> {
    let hostile = ["://", "*", "$", ";", " ", "`", "|", "&"]
        .iter()
        .any(|token| value.contains(token));
    if value.trim().is_empty() || hostile {
        return Err(invalid_input(field, value));
    }
    Ok(())
}

/// Reject blank artifact names and names outside the token charset.
fn check_artifact(artifact: &str) -> Result<(), MiseError> {
    let valid = !artifact.is_empty()
        && artifact
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if valid {
        Ok(())
    } else {
        Err(invalid_input("artifact", artifact))
    }
}

/// Build the shared rejection for a malformed lookup input.
fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidBaselineInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
