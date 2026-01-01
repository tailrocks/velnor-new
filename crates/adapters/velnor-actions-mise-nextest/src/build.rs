//! Fixed candidate-build vector through pinned Rust and action-owned MBX (boot §4).
//!
//! The candidate compiles `velnor-actions-cli` with the exact pinned
//! toolchain; one byte-exact shape serves the candidate build and the
//! pre-seed helper build. Global flags ride before the subcommand per
//! the isolated wrapper, matching the contract spelling (`mise
//! --no-config exec ...` — mise rejects flags after the subcommand).

use std::ffi::OsString;

use velnor_actions_mise_catalog::catalog::{PinnedTool, ToolCatalog};
use velnor_actions_mise_catalog::requests::PinnedToolExec;
use velnor_actions_mise_core::command::IsolatedCommand;
use velnor_actions_mise_core::error::MiseError;

/// Package compiled by the candidate build.
pub const CANDIDATE_BUILD_PACKAGE: &str = "velnor-actions-cli";

/// Binary compiled by the candidate build.
pub const CANDIDATE_BUILD_BIN: &str = "velnor-actions";

/// Payload program handling compilation.
const MBX_PROGRAM: &str = "mbx";

/// Fixed build arguments after the program.
const BUILD_ARGS: [&str; 7] = [
    "build",
    "--release",
    "--locked",
    "--package",
    CANDIDATE_BUILD_PACKAGE,
    "--bin",
    CANDIDATE_BUILD_BIN,
];

/// Candidate compilation under exact Rust; the native action owns MBX on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBuild {
    /// Fixed pinned-Rust execution: action-owned `mbx build --release --locked ...`.
    exec: PinnedToolExec,
}

impl CandidateBuild {
    /// Build the fixed candidate compilation.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::ForbiddenPayload`] only if the fixed payload
    /// were forbidden, which construction rules out.
    pub fn new() -> Result<Self, MiseError> {
        let args = BUILD_ARGS.iter().map(OsString::from).collect();
        Ok(Self {
            exec: PinnedToolExec::new(
                vec![PinnedTool::Rust],
                std::ffi::OsStr::new(MBX_PROGRAM),
                args,
            )?,
        })
    }

    /// Tools selected as `<tool>@<exact>` before the `--` separator.
    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        self.exec.tools()
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.exec.argv(catalog)
    }

    /// Isolated command running this build.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.exec.command(catalog)
    }
}
