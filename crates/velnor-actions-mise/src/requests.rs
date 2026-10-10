//! Typed mise requests: metadata discovery, qualification, and pinned exec.
//!
//! Metadata discovery and locked qualification run through pinned MBX, including
//! for workspaces that do not compile. Discovery never resolves (`--no-deps`:
//! no fetch, no write); only lockful qualification resolves, `--locked
//! --offline`. Callers own parsing; discovery and qualification return the raw
//! metadata JSON string, and pinned exec returns the typed output.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{
    IsolatedCommand, ProcessOutput, mise_argv_tail, mise_install_argv_tail, redact_argv_for_debug,
};
use crate::error::MiseError;

/// MBX payload program executed after the `--` separator.
const MBX_PROGRAM: &str = "mbx";
/// Cargo payload name retained only for rejecting source-install commands.
const CARGO_PROGRAM: &str = "cargo";

/// MBX's exact Rust toolchain selector.
fn mbx_rust_selector(catalog: &ToolCatalog) -> String {
    // Kept as a helper so metadata requests and MBX's own command-line
    // selector cannot diverge from the Rust catalog identity.
    format!("+{}", catalog.version(PinnedTool::Rust))
}

/// MBX subcommand reporting workspace metadata as JSON.
const METADATA_SUBCOMMAND: &str = "metadata";

fn metadata_mbx_argv(
    catalog: &ToolCatalog,
    manifest: &Path,
    resolution_flags: &[&str],
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from(MBX_PROGRAM),
        OsString::from(mbx_rust_selector(catalog)),
        OsString::from(METADATA_SUBCOMMAND),
        OsString::from("--format-version"),
        OsString::from("1"),
    ];
    argv.extend(resolution_flags.iter().map(OsString::from));
    argv.extend([
        OsString::from("--manifest-path"),
        manifest.as_os_str().to_owned(),
    ]);
    argv
}

/// Conservative discovery of one manifest through pinned MBX.
///
/// Exact payload: `mbx +<rust> metadata --format-version 1 --no-deps
/// --manifest-path <manifest>`. No `--locked`/`--offline`: discovery must not
/// wait for full resolution. `--no-deps` skips resolution entirely, so the
/// probe performs no index access, network fetch, or repository write --
/// not even for lockless-with-dependencies manifests (poison-fixture proven;
/// the orchestrator also brackets every run with a tool snapshot that fails
/// closed on drift). Full resolution is qualification's job, lockful-only.
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

    /// MBX payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn mbx_argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        metadata_mbx_argv(catalog, &self.manifest, &["--no-deps"])
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(
            catalog,
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            &self.mbx_argv(catalog),
        )
    }

    /// Isolated command running this discovery.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]);
        Ok(IsolatedCommand::mise_exec(&specs, &self.mbx_argv(catalog))?
            .with_policy(crate::command::EnvPolicy::Mbx))
    }

    /// Run discovery and return the raw metadata JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when MBX cannot launch,
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
/// Exact payload: `mbx +<rust> metadata --format-version 1 --locked --offline
/// --manifest-path <workspace-root>/Cargo.toml`. Missing offline
/// dependencies surface as `preparation_incomplete` upstream, never a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataQualification {
    /// Workspace-root manifest whose resolution is qualified.
    workspace_manifest: PathBuf,
}

impl MetadataQualification {
    /// Qualify resolution for one workspace-root manifest.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn new(workspace_manifest: PathBuf) -> Result<Self, MiseError> {
        if workspace_manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { workspace_manifest })
    }

    /// Workspace-root manifest whose resolution is qualified.
    #[must_use]
    pub fn workspace_manifest(&self) -> &Path {
        &self.workspace_manifest
    }

    /// MBX payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn mbx_argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        metadata_mbx_argv(
            catalog,
            &self.workspace_manifest,
            &["--locked", "--offline"],
        )
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(
            catalog,
            &[PinnedTool::Rust, PinnedTool::MrBoxington],
            &self.mbx_argv(catalog),
        )
    }

    /// Isolated command running this qualification.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]);
        Ok(IsolatedCommand::mise_exec(&specs, &self.mbx_argv(catalog))?
            .with_policy(crate::command::EnvPolicy::Mbx))
    }

    /// Run qualification and return the raw metadata JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when MBX cannot launch,
    /// [`MiseError::NonZeroExit`] on nonzero status, and
    /// [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}

/// One payload program run under at least one pinned tool.
///
/// `Debug` redacts `--token` values; payload shape stays visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MbxAuthority {
    /// MBX is selected from the catalog by the enclosing Mise command.
    Catalog,
    /// A typed action-owned route supplies MBX through its pinned action.
    ActionOwned,
}

/// Typed exact-pinned tool execution that rejects payloads bypassing Mise.
#[derive(Clone, PartialEq, Eq)]
pub struct PinnedToolExec {
    /// Tools selected as `<tool>@<exact>` before the `--` separator.
    tools: Vec<PinnedTool>,
    /// Payload program executed after the separator.
    program: OsString,
    /// Payload arguments passed byte-exact.
    args: Vec<OsString>,
    /// Explicit authority for an MBX payload; arbitrary payloads cannot
    /// silently resolve an ambient `mbx` executable.
    mbx_authority: Option<MbxAuthority>,
}

impl std::fmt::Debug for PinnedToolExec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedToolExec")
            .field("tools", &self.tools)
            .field("program", &self.program)
            .field("args", &redact_argv_for_debug(&self.args))
            .field("mbx_authority", &self.mbx_authority)
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
    /// validation runs through Mise-selected tools only. An `mbx` payload is
    /// accepted only as the bare executable name with both the Rust and MBX
    /// catalog tools selected; path-qualified MBX executables are never
    /// treated as catalog-pinned.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools,
    /// [`MiseError::EmptyCommand`] for an empty program, and
    /// [`MiseError::ForbiddenPayload`] for `rustup` programs, `cargo install`
    /// payloads, and MBX payloads without exact Rust and MBX catalog selectors.
    pub fn new(
        tools: Vec<PinnedTool>,
        program: &OsStr,
        args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        let mbx_authority = if is_mbx_program(program) {
            if program != OsStr::new("mbx")
                || !tools.contains(&PinnedTool::Rust)
                || !tools.contains(&PinnedTool::MrBoxington)
            {
                return Err(unpinned_mbx_error(program));
            }
            Some(MbxAuthority::Catalog)
        } else {
            None
        };
        Self::new_with_authority(tools, program, args, mbx_authority)
    }

    /// Build a fixed candidate through Rust selected by Mise and MBX supplied
    /// by the workflow's separately pinned native action.
    pub(crate) fn new_action_owned_mbx(
        tools: Vec<PinnedTool>,
        program: &OsStr,
        args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        if program != OsStr::new("mbx")
            || !tools.contains(&PinnedTool::Rust)
            || tools.contains(&PinnedTool::MrBoxington)
        {
            return Err(unpinned_mbx_error(program));
        }
        Self::new_with_authority(tools, program, args, Some(MbxAuthority::ActionOwned))
    }

    fn new_with_authority(
        tools: Vec<PinnedTool>,
        program: &OsStr,
        args: Vec<OsString>,
        mbx_authority: Option<MbxAuthority>,
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
            mbx_authority,
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

    /// Isolated command running this pinned execution (`gh` selects Baseline).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the payload were empty,
    /// which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        let command = IsolatedCommand::mise_exec(&specs, &self.payload())?;
        if self.tools.as_slice() == [PinnedTool::Gh] {
            return Ok(command.with_policy(crate::command::EnvPolicy::Baseline));
        }
        if self.mbx_authority.is_some() {
            return Ok(command.with_policy(crate::command::EnvPolicy::Mbx));
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

fn is_mbx_program(program: &OsStr) -> bool {
    Path::new(program)
        .file_stem()
        .is_some_and(|stem| stem == "mbx")
}

fn unpinned_mbx_error(program: &OsStr) -> MiseError {
    MiseError::ForbiddenPayload {
        program: program.to_string_lossy().into_owned(),
        reason: "mbx_requires_catalog_or_action_authority".to_owned(),
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

/// Reject `rustup` payloads (bare or absolute) and `cargo install` payloads.
fn reject_forbidden_payload(program: &OsStr, args: &[OsString]) -> Result<(), MiseError> {
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
) -> Vec<OsString> {
    let specs = catalog.tool_specs(tools);
    let mut argv = vec![OsString::from("mise")];
    argv.extend(mise_argv_tail("exec", &specs, payload));
    argv
}
