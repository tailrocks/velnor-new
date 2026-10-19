//! Typed mise requests: metadata discovery, qualification, and pinned exec.
//!
//! Discovery uses Cargo even for MBX workspaces (metadata discovery is not
//! compilation). Discovery never resolves (`--no-deps`: no fetch, no write);
//! only lockful qualification resolves, `--locked --offline`. Callers own
//! parsing; discovery and qualification return the raw metadata JSON string,
//! and pinned exec returns the typed output.

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{
    IsolatedCommand, ProcessOutput, mise_install_argv_tail, redact_argv_for_debug,
};
use crate::error::MiseError;
use std::ffi::{OsStr, OsString};
#[path = "requests_helpers.rs"]
mod helpers;
use helpers::full_mise_argv;
#[path = "requests_metadata.rs"]
mod metadata;
pub use metadata::{MetadataDiscovery, MetadataQualification};

/// One payload program run under at least one pinned tool.
///
/// `Debug` redacts `--token` values; payload shape stays visible.
#[derive(Clone, PartialEq, Eq)]
pub struct PinnedToolExec {
    /// Tools selected as `<tool>@<exact>` before the `--` separator.
    tools: Vec<PinnedTool>,
    /// Payload program executed after the separator.
    program: OsString,
    /// Payload arguments passed byte-exact.
    args: Vec<OsString>,
}

impl std::fmt::Debug for PinnedToolExec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedToolExec")
            .field("tools", &self.tools)
            .field("program", &self.program)
            .field("args", &redact_argv_for_debug(&self.args))
            .finish()
    }
}

/// Program stem that must never run as a payload: direct toolchain
/// management bypasses Mise selection, so it is forbidden.
const FORBIDDEN_PROGRAM_STEM: &str = "rustup";

impl PinnedToolExec {
    /// Run `program` with `args` under the given pinned tools.
    ///
    /// Direct toolchain managers and installer actions are rejected:
    /// validation runs through Mise-selected tools only.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools,
    /// [`MiseError::EmptyCommand`] for an empty program, and
    /// [`MiseError::ForbiddenPayload`] for `rustup` programs and
    /// `cargo install` payloads.
    pub fn new(
        tools: Vec<PinnedTool>,
        program: &OsStr,
        args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        if program.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        }
        reject_forbidden_payload(program, &args)?;
        Ok(Self {
            tools,
            program: program.to_owned(),
            args,
        })
    }

    /// Tools selected as `<tool>@<exact>` before the `--` separator.
    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        &self.tools
    }

    /// Payload program executed after the separator.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Payload arguments passed byte-exact.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Payload arguments: program first, then its arguments.
    #[must_use]
    pub fn payload(&self) -> Vec<OsString> {
        let mut payload = Vec::with_capacity(self.args.len() + 1);
        payload.push(self.program.clone());
        payload.extend(self.args.iter().cloned());
        payload
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &self.tools, &self.payload())
    }

    /// Isolated command running this pinned execution (`gh` selects `GhApi`).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the payload were empty,
    /// which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        let command = IsolatedCommand::mise_exec(&specs, &self.payload())?;
        if self.tools.as_slice() == [PinnedTool::Gh] {
            return Ok(command.with_policy(crate::command::EnvPolicy::GhApi));
        }
        Ok(command)
    }

    /// Build a pinned GitHub CLI command using one explicitly selected token.
    ///
    /// Only the `gh` pinned tool may receive this credential. The token is
    /// passed through the child environment and never through argv.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for non-`gh` tools or malformed tokens.
    pub fn command_with_github_token(
        &self,
        catalog: &ToolCatalog,
        token: &str,
    ) -> Result<IsolatedCommand, MiseError> {
        if self.tools.as_slice() != [PinnedTool::Gh] || self.program != "gh" {
            return Err(MiseError::InvalidStepInput {
                field: "GH_TOKEN".to_owned(),
                value: "credential_requires_pinned_gh".to_owned(),
            });
        }
        self.command(catalog)?.with_github_token(token)
    }

    /// Run the payload and return its typed output, whatever the exit is.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when the child cannot be
    /// spawned or reaped. A nonzero exit is returned as data.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<ProcessOutput, MiseError> {
        self.command(catalog)?.run()
    }
}

/// Reject `rustup` payloads (bare or absolute) and `cargo install` payloads.
fn reject_forbidden_payload(program: &OsStr, args: &[OsString]) -> Result<(), MiseError> {
    let stem = std::path::Path::new(program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if stem == FORBIDDEN_PROGRAM_STEM {
        return Err(MiseError::ForbiddenPayload {
            program: program.to_string_lossy().into_owned(),
            reason: "rustup_forbidden".to_owned(),
        });
    }
    let is_cargo_install =
        program == OsStr::new("cargo") && args.first().is_some_and(|arg| arg == "install");
    if is_cargo_install {
        return Err(MiseError::ForbiddenPayload {
            program: "cargo".to_owned(),
            reason: "cargo_install_forbidden".to_owned(),
        });
    }
    Ok(())
}

/// Bootstrap installation of exact catalog tools (the `mise install` step).
///
/// Exact argv: `mise --no-config --no-env --no-hooks install
/// <tool>@<exact>...` plus Velnor-owned homes, under the full isolation
/// overlay. No repo config ever loads: explicit specs are the sole
/// version authority. This is the bootstrap exception: the only Velnor
/// invocation that installs tools. Every later invocation runs through
/// `exec` with implicit installation disabled, so a missing tool fails
/// as a preparation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseInstall {
    /// Tools installed as `<tool>@<exact>` selectors.
    tools: Vec<PinnedTool>,
}

impl MiseInstall {
    /// Install the given catalog tools at their exact pins.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools.
    pub fn new(tools: Vec<PinnedTool>) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Ok(Self { tools })
    }

    /// Tools installed as `<tool>@<exact>` selectors.
    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        &self.tools
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&self.tools);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_install_argv_tail(&specs));
        argv
    }

    /// Isolated command running this installation.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        IsolatedCommand::mise_install(&specs)
    }

    /// Run the installation and return its typed output.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when the child cannot be
    /// spawned or reaped. A nonzero exit is returned as data.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<ProcessOutput, MiseError> {
        self.command(catalog)?.run()
    }
}
