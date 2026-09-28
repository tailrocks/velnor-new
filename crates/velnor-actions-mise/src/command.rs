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

/// Program name for mise invocations.
const MISE_PROGRAM: &str = "mise";

/// Subcommand selecting pinned tools for one payload invocation.
const MISE_EXEC_SUBCOMMAND: &str = "exec";

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
}

impl IsolatedCommand {
    /// Build `mise <globals> exec <specs> -- <payload>`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] when the payload is empty.
    pub fn mise_exec(tool_specs: &[String], payload: &[OsString]) -> Result<Self, MiseError> {
        if payload.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: MISE_PROGRAM.to_owned(),
            });
        }
        Ok(Self {
            program: OsString::from(MISE_PROGRAM),
            args: mise_argv_tail(tool_specs, payload),
            cwd: None,
        })
    }

    /// Build a direct (non-mise) invocation with the same isolation env.
    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
        }
    }

    /// Override the working directory for this invocation.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
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
        ISOLATION_ENV
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
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
        for (key, value) in Self::env_overlay() {
            command.env(key, value);
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command.stdin(Stdio::null());
        command
    }
}

/// Assemble the mise argument tail: globals, `exec`, specs, `--`, payload.
pub(crate) fn mise_argv_tail(tool_specs: &[String], payload: &[OsString]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(tool_specs.len() + payload.len() + 6);
    for flag in MISE_GLOBAL_FLAGS {
        args.push(OsString::from(flag));
    }
    args.push(OsString::from(MISE_EXEC_SUBCOMMAND));
    for spec in tool_specs {
        args.push(OsString::from(spec));
    }
    args.push(OsString::from(TOOL_COMMAND_SEPARATOR));
    args.extend(payload.iter().cloned());
    args
}
