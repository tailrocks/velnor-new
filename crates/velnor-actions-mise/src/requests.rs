//! Typed mise requests: metadata discovery, qualification, and pinned exec.
//!
//! Discovery uses Cargo even for MBX workspaces (metadata discovery is not
//! compilation). Discovery never resolves (`--no-deps`: no fetch, no write);
//! only lockful qualification resolves, `--locked --offline`. Callers own
//! parsing; discovery and qualification return the raw metadata JSON string,
//! and pinned exec returns the typed output.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{
    IsolatedCommand, ProcessOutput, mise_argv_tail, mise_install_argv_tail, redact_argv_for_debug,
};
use crate::error::MiseError;

mod host;
mod qualification;
mod runtime;

/// Cargo payload program executed after the `--` separator.
const CARGO_PROGRAM: &str = "cargo";

/// Cargo subcommand reporting workspace metadata as JSON.
const CARGO_METADATA: &str = "metadata";

/// Conservative discovery of one manifest through pinned Cargo.
///
/// Exact payload: `cargo metadata --format-version 1 --locked --no-deps
/// --manifest-path <manifest>`. `--locked` preserves the locked-execution
/// policy; `--no-deps` skips resolution entirely, so the
/// probe performs no index access, network fetch, or repository write --
/// not even for lockless-with-dependencies manifests (poison-fixture proven;
/// the orchestrator also brackets every run with a tool snapshot that fails
/// closed on drift). This does not prove lockfile presence or resolution;
/// full resolution is qualification's job, lockful-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDiscovery {
    /// Manifest whose metadata is requested.
    manifest: PathBuf,
}

impl MetadataDiscovery {
    /// Discover metadata for one manifest path.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn new(manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { manifest })
    }

    /// Manifest whose metadata is requested.
    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// Cargo-side payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn cargo_argv(&self) -> Vec<OsString> {
        vec![
            OsString::from(CARGO_PROGRAM),
            OsString::from(CARGO_METADATA),
            OsString::from("--format-version"),
            OsString::from("1"),
            OsString::from("--locked"),
            OsString::from("--no-deps"),
            OsString::from("--manifest-path"),
            self.manifest.as_os_str().to_owned(),
        ]
    }

    /// Full mise argument vector including the program.
    /// # Errors
    /// Rejects native selectors without explicit host qualification.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        full_mise_argv(catalog, &metadata_tools(catalog), &self.cargo_argv())
    }

    /// Isolated command running this discovery.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&metadata_tools(catalog))?;
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    /// Run discovery and return the raw metadata JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when Cargo cannot launch,
    /// [`MiseError::NonZeroExit`] on nonzero status, and
    /// [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}

/// Locked/offline qualification after dependency sources have been prepared.
///
/// Exact payload: `cargo metadata --format-version 1 --locked --offline
/// --manifest-path <workspace-root>/Cargo.toml`. Missing offline
/// dependencies surface as `preparation_incomplete` upstream, never a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataQualification {
    /// Workspace-root manifest whose resolution is qualified.
    workspace_manifest: PathBuf,
}

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
    /// # Errors
    /// Rejects native selectors without explicit host qualification.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        full_mise_argv(catalog, &self.tools, &self.payload())
    }

    /// Isolated command running this pinned execution (`gh` selects Baseline).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the payload were empty,
    /// which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools)?;
        let command = IsolatedCommand::mise_exec(&specs, &self.payload())?;
        if self.tools.as_slice() == [PinnedTool::Gh] {
            return Ok(command.with_policy(crate::command::EnvPolicy::Baseline));
        }
        Ok(command)
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
    /// # Errors
    /// Rejects native selectors without explicit host qualification.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        let specs = catalog.tool_specs(&self.tools)?;
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_install_argv_tail(&specs));
        Ok(argv)
    }

    /// Isolated command running this installation.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools)?;
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

/// Reject `rustup` payloads (bare or absolute) and `cargo install` payloads.
pub(crate) fn reject_forbidden_payload(
    program: &OsStr,
    args: &[OsString],
) -> Result<(), MiseError> {
    let stem = Path::new(program)
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
        program == OsStr::new(CARGO_PROGRAM) && args.first().is_some_and(|arg| arg == "install");
    if is_cargo_install {
        return Err(MiseError::ForbiddenPayload {
            program: CARGO_PROGRAM.to_owned(),
            reason: "cargo_install_forbidden".to_owned(),
        });
    }
    Ok(())
}

/// Full mise argv: program plus the shared globals/`exec`/specs/`--` tail.
fn full_mise_argv(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    payload: &[OsString],
) -> Result<Vec<OsString>, MiseError> {
    let specs = catalog.tool_specs(tools)?;
    let mut argv = vec![OsString::from("mise")];
    argv.extend(mise_argv_tail("exec", &specs, payload));
    Ok(argv)
}

fn metadata_tools(catalog: &ToolCatalog) -> Vec<PinnedTool> {
    let mut tools = vec![catalog.compiler_tool()];
    if catalog.rust_uses_mbx() {
        tools.push(PinnedTool::MrBoxington);
    }
    tools
}
