//! Fixed subprocess wrapper: the sole `std::process::Command` constructor.
//!
//! Every spawned process inherits isolation from one place: the `MISE_*`
//! environment quartet, mise global flags before the subcommand, a `--`
//! separator between tool selectors and the payload, and byte-exact argument
//! passthrough. No shell is ever involved.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::MiseError;

/// Isolation environment applied to every spawned process.
///
/// `MISE_LOCKFILE=0` disables lockfile maintenance; the `MISE_NO_*` trio
/// blocks project config, env files, and hooks.
pub const ISOLATION_ENV: [(&str, &str); 4] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
];

/// Mise global flags, always placed before the subcommand.
pub const MISE_GLOBAL_FLAGS: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];

/// Separator between mise tool selectors and the payload command.
pub const TOOL_COMMAND_SEPARATOR: &str = "--";

/// Mise subcommands Velnor may emit: `exec` for every verification and
/// payload run, `install` for the bootstrap prepare step only, `run` for
/// Gate-6 qualified task-cache invocations only. Tool-file management
/// subcommands are never emitted.
pub const ALLOWED_MISE_SUBCOMMANDS: [&str; 3] = ["exec", "install", "run"];

/// Environment disabling implicit tool installation for verification runs.
///
/// Qualified against mise 2026.9.14 (`mise settings ls -a`): `auto_install`
/// and `exec_auto_install` both default to true. With both false, a missing
/// tool fails the invocation instead of installing, so the failure surfaces
/// as a preparation error. Explicit `mise install` still installs.
pub const NO_AUTO_INSTALL_ENV: [(&str, &str); 2] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
];

/// Environment name for the Velnor-owned mise Rust home.
pub const MISE_RUSTUP_HOME_ENV: &str = "MISE_RUSTUP_HOME";

/// Environment name for the Velnor-owned mise Cargo home.
pub const MISE_CARGO_HOME_ENV: &str = "MISE_CARGO_HOME";

/// Environment name selecting the exact Rust toolchain for Cargo runs.
///
/// Rustup gives this override precedence over any directory toolchain file.
pub const RUSTUP_TOOLCHAIN_ENV: &str = "RUSTUP_TOOLCHAIN";

/// Program name for mise invocations.
const MISE_PROGRAM: &str = "mise";

/// Subcommand selecting pinned tools for one payload invocation.
const MISE_EXEC_SUBCOMMAND: &str = "exec";

/// Subcommand installing exact tool versions for the bootstrap prepare step.
const MISE_INSTALL_SUBCOMMAND: &str = "install";

/// Whether a subcommand is inside the Velnor mise allowlist.
#[must_use]
pub fn is_allowed_mise_subcommand(subcommand: &str) -> bool {
    ALLOWED_MISE_SUBCOMMANDS.contains(&subcommand)
}

/// Typed child-process result: captured streams plus a typed exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Captured standard output bytes.
    pub stdout: Vec<u8>,
    /// Captured standard error bytes.
    pub stderr: Vec<u8>,
    /// Exit code when the platform reports one.
    pub code: Option<i32>,
    /// Whether the exit status reports success.
    pub success: bool,
}

impl ProcessOutput {
    /// Fail with [`MiseError::NonZeroExit`] unless the status reports success.
    ///
    /// # Errors
    ///
    /// Returns the typed exit failure when `success` is false.
    pub fn require_success(&self, program: &str) -> Result<&Self, MiseError> {
        if self.success {
            Ok(self)
        } else {
            Err(MiseError::NonZeroExit {
                program: program.to_owned(),
                code: self.code,
                stderr: String::from_utf8_lossy(&self.stderr).into_owned(),
            })
        }
    }

    /// Decode standard output as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidUtf8`] when stdout is not valid UTF-8.
    pub fn stdout_text(&self, program: &str) -> Result<String, MiseError> {
        String::from_utf8(self.stdout.clone()).map_err(|_| MiseError::InvalidUtf8 {
            program: program.to_owned(),
            stream: "stdout".to_owned(),
        })
    }
}

/// A fully isolated, shell-free child-process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedCommand {
    /// Program executed directly (never through a shell).
    program: OsString,
    /// Arguments passed byte-exact to the program.
    args: Vec<OsString>,
    /// Working directory override; none inherits the parent directory.
    cwd: Option<PathBuf>,
    /// Extra environment pairs applied after [`Self::env_overlay`].
    extra_env: Vec<(OsString, OsString)>,
}

impl IsolatedCommand {
    /// Build `mise <globals> exec <specs> -- <payload>`.
    ///
    /// Implicit tool installation is disabled: a missing tool fails instead
    /// of installing, so verification surfaces preparation errors.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] when the payload is empty.
    pub fn mise_exec(tool_specs: &[String], payload: &[OsString]) -> Result<Self, MiseError> {
        Self::mise_with_subcommand(MISE_EXEC_SUBCOMMAND, tool_specs, Some(payload), true)
    }

    /// Build `mise <globals> install <specs>` for the bootstrap prepare step.
    ///
    /// The only Velnor invocation that installs tools. It carries the
    /// isolation quartet but no install disable, so explicit installation
    /// always proceeds.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] when no tool is named.
    pub fn mise_install(tool_specs: &[String]) -> Result<Self, MiseError> {
        if tool_specs.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Self::mise_with_subcommand(MISE_INSTALL_SUBCOMMAND, tool_specs, None, false)
    }

    /// Build a direct (non-mise) invocation with the same isolation env.
    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env: Vec::new(),
        }
    }

    /// Override the working directory for this invocation.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Append extra environment pairs applied after the isolation overlay.
    #[must_use]
    pub fn with_env(mut self, extra: &[(OsString, OsString)]) -> Self {
        self.extra_env.extend(extra.iter().cloned());
        self
    }

    /// Program executed directly.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments passed byte-exact to the program.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Working directory override when set.
    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Full argument vector including the program, for renderers and tests.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.clone());
        argv.extend(self.args.iter().cloned());
        argv
    }

    /// Environment overlay applied on top of the inherited environment.
    ///
    /// This is the single source read by the spawner, so tests observing it
    /// cannot drift from what children receive.
    #[must_use]
    pub fn env_overlay() -> Vec<(OsString, OsString)> {
        pairs_of(&ISOLATION_ENV)
    }

    /// Extra environment pairs appended via [`Self::with_env`] or the
    /// constructor (verification runs carry the install disable here).
    #[must_use]
    pub fn extra_env(&self) -> &[(OsString, OsString)] {
        &self.extra_env
    }

    /// Full environment the spawner applies: overlay first, then extras.
    ///
    /// This is the single source read by the spawner, so tests observing it
    /// cannot drift from what children receive.
    #[must_use]
    pub fn full_env(&self) -> Vec<(OsString, OsString)> {
        let mut env = Self::env_overlay();
        env.extend(self.extra_env.iter().cloned());
        env
    }

    /// Spawn the child, capture both streams, and wait for its typed exit.
    ///
    /// Standard input is closed; no shell interprets any argument.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when the child cannot be
    /// spawned or reaped. A nonzero exit is returned as data, not an error.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        let output = self
            .command()
            .output()
            .map_err(|err| MiseError::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                message: err.to_string(),
            })?;
        Ok(ProcessOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            code: output.status.code(),
            success: output.status.success(),
        })
    }

    /// The one `Command` constructor: direct spawn, piped streams, no shell.
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        for (key, value) in self.full_env() {
            command.env(key, value);
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command.stdin(Stdio::null());
        command
    }

    /// Build a mise invocation for an allowlisted subcommand.
    ///
    /// `exec` takes a payload after `--` and disables implicit installs;
    /// `install` takes specs only. Every call site passes a constant from
    /// [`ALLOWED_MISE_SUBCOMMANDS`]; the predicate pins that surface.
    fn mise_with_subcommand(
        subcommand: &str,
        tool_specs: &[String],
        payload: Option<&[OsString]>,
        disable_install: bool,
    ) -> Result<Self, MiseError> {
        debug_assert!(is_allowed_mise_subcommand(subcommand));
        let args = match payload {
            Some(payload) => {
                if payload.is_empty() {
                    return Err(MiseError::EmptyCommand {
                        program: MISE_PROGRAM.to_owned(),
                    });
                }
                mise_argv_tail(subcommand, tool_specs, payload)
            }
            None => mise_install_argv_tail(tool_specs),
        };
        let extra_env = if disable_install {
            pairs_of(&NO_AUTO_INSTALL_ENV)
        } else {
            Vec::new()
        };
        Ok(Self {
            program: OsString::from(MISE_PROGRAM),
            args,
            cwd: None,
            extra_env,
        })
    }
}

/// Velnor-owned toolchain environment for one Cargo invocation.
///
/// `rustup_home` and `cargo_home` are caller-supplied Velnor-owned persistent
/// paths; `toolchain` is the exact pinned Rust version from the catalog.
/// Renderers embed these pairs in generated steps; local spawns append them
/// via [`IsolatedCommand::with_env`].
#[must_use]
pub fn toolchain_env(
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Vec<(OsString, OsString)> {
    [
        (MISE_RUSTUP_HOME_ENV, rustup_home),
        (MISE_CARGO_HOME_ENV, cargo_home),
        (RUSTUP_TOOLCHAIN_ENV, toolchain),
    ]
    .iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

/// Convert a static string table into owned environment pairs.
fn pairs_of<const N: usize>(table: &[(&str, &str); N]) -> Vec<(OsString, OsString)> {
    table
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

/// Assemble the mise argument tail: globals, subcommand, specs, `--`, payload.
pub(crate) fn mise_argv_tail(
    subcommand: &str,
    tool_specs: &[String],
    payload: &[OsString],
) -> Vec<OsString> {
    let mut args = Vec::with_capacity(tool_specs.len() + payload.len() + 6);
    for flag in MISE_GLOBAL_FLAGS {
        args.push(OsString::from(flag));
    }
    args.push(OsString::from(subcommand));
    for spec in tool_specs {
        args.push(OsString::from(spec));
    }
    args.push(OsString::from(TOOL_COMMAND_SEPARATOR));
    args.extend(payload.iter().cloned());
    args
}

/// Assemble the install tail: globals, `install`, then exact specs only.
pub(crate) fn mise_install_argv_tail(tool_specs: &[String]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(tool_specs.len() + 4);
    for flag in MISE_GLOBAL_FLAGS {
        args.push(OsString::from(flag));
    }
    args.push(OsString::from(MISE_INSTALL_SUBCOMMAND));
    for spec in tool_specs {
        args.push(OsString::from(spec));
    }
    args
}
