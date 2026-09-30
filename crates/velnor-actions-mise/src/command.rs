//! Fixed subprocess wrapper: the sole `std::process::Command` constructor.
//! Trusted tooling inherits the parent env; repo-task children spawn cleared.

//! Environment policy lives in `command_env.rs` and the typed result in
//! `command_output.rs`; both are declared here so `lib.rs` stays untouched.
#[path = "command_env.rs"]
mod env;
#[path = "command_output.rs"]
mod output;

pub use self::env::{
    EnvPolicy, ISOLATION_ENV, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, NO_AUTO_INSTALL_ENV,
    RUSTUP_TOOLCHAIN_ENV, is_reserved_env_key, toolchain_env,
};
use self::env::{pairs_of, redact_env_for_debug};
pub(crate) use self::output::redact_argv_for_debug;
pub use self::output::{
    ProcessOutput, SPAWN_CANCELLED_MESSAGE, SPAWN_TIMEOUT_MESSAGE_PREFIX, is_cancel_or_timeout,
};
use self::output::{read_capped, signal_of};

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::MiseError;

/// Mise global flags, always placed before the subcommand.
pub const MISE_GLOBAL_FLAGS: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];

/// Separator between mise tool selectors and the payload command.
pub const TOOL_COMMAND_SEPARATOR: &str = "--";

/// Mise subcommands Velnor may emit: `exec`, `install`, and `run`.
pub const ALLOWED_MISE_SUBCOMMANDS: [&str; 3] = ["exec", "install", "run"];

/// Captured bytes kept per stream; past this the run fails closed.
pub const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// Default run deadline in seconds; past this the child is killed.
pub const RUN_TIMEOUT_SECS: u64 = 600;

/// Whether a subcommand is inside the Velnor mise allowlist.
#[must_use]
pub fn is_allowed_mise_subcommand(subcommand: &str) -> bool {
    ALLOWED_MISE_SUBCOMMANDS.contains(&subcommand)
}

/// External cancellation handle for one subprocess run (P07-5).
///
/// Beyond the internal deadline: any thread holding a shared handle may
/// abort the run, and the child is killed at the next poll. Cancellation
/// surfaces as typed [`MiseError::SpawnFailed`], never as a task outcome.
#[derive(Debug, Default)]
pub struct CancelHandle {
    /// Cancellation flag shared with the polling run loop.
    cancelled: AtomicBool,
}

impl CancelHandle {
    /// New uncancelled handle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation; the run loop kills the child at its next poll.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A fully isolated, shell-free child-process invocation.
///
/// `Debug` redacts `--token` values and secret-looking env values so
/// logs never carry credential material; shape stays visible.
#[derive(Clone, PartialEq, Eq)]
pub struct IsolatedCommand {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    extra_env: Vec<(OsString, OsString)>,
    policy: EnvPolicy,
}

impl std::fmt::Debug for IsolatedCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IsolatedCommand")
            .field("program", &self.program)
            .field("args", &redact_argv_for_debug(&self.args))
            .field("cwd", &self.cwd)
            .field("extra_env", &redact_env_for_debug(&self.extra_env))
            .field("policy", &self.policy)
            .finish()
    }
}

impl IsolatedCommand {
    /// Build `mise <globals> exec <specs> -- <payload>`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] when `payload` is empty.
    pub fn mise_exec(tool_specs: &[String], payload: &[OsString]) -> Result<Self, MiseError> {
        Self::mise_with_subcommand("exec", tool_specs, Some(payload), true)
    }

    /// Build `mise <globals> install <specs>`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] when `tool_specs` is empty.
    pub fn mise_install(tool_specs: &[String]) -> Result<Self, MiseError> {
        if tool_specs.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Self::mise_with_subcommand("install", tool_specs, None, false)
    }

    pub(crate) fn direct(program: &str, args: Vec<OsString>) -> Self {
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env: Vec::new(),
            policy: EnvPolicy::Discovery,
        }
    }

    /// Build a repo-task child: cleared env plus explicit declared inputs.
    /// Reserved keys are rejected; platform values arrive only as declared inputs.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] when a declared key is reserved.
    pub fn repo_task(
        program: &str,
        args: Vec<OsString>,
        declared: &[(OsString, OsString)],
    ) -> Result<Self, MiseError> {
        for (key, _) in declared {
            if is_reserved_env_key(&key.to_string_lossy()) {
                return Err(MiseError::InvalidStepInput {
                    field: key.to_string_lossy().into_owned(),
                    value: "reserved_env_key".to_owned(),
                });
            }
        }
        Ok(Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env: declared.to_vec(),
            policy: EnvPolicy::RepoTask,
        })
    }

    /// Override the working directory for this invocation.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Append extras; reserved keys are dropped fail-closed at construction.
    #[must_use]
    pub fn with_env(mut self, extra: &[(OsString, OsString)]) -> Self {
        for pair in extra {
            if !is_reserved_env_key(&pair.0.to_string_lossy()) {
                self.extra_env.push(pair.clone());
            }
        }
        self
    }

    /// Program executed directly.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
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

    /// Environment overlay applied by the spawner; tests read this.
    #[must_use]
    pub fn env_overlay() -> Vec<(OsString, OsString)> {
        pairs_of(&ISOLATION_ENV)
    }

    /// Full environment the spawner applies: overlay first, then extras.
    #[must_use]
    pub fn full_env(&self) -> Vec<(OsString, OsString)> {
        let mut env = Self::env_overlay();
        env.extend(self.extra_env.iter().cloned());
        env
    }

    /// Whether implicit installation is disabled (effective last-wins value).
    #[must_use]
    pub fn disables_auto_install(&self) -> bool {
        let full = self.full_env();
        NO_AUTO_INSTALL_ENV.iter().all(|(key, value)| {
            full.iter()
                .rev()
                .find(|(found, _)| found == key)
                .is_some_and(|(_, seen)| seen == value)
        })
    }

    /// Spawn the child under default bounds and wait for its typed exit.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] on spawn failure, output-limit
    /// breach, reader failure, or timeout.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        let timeout = Duration::from_secs(RUN_TIMEOUT_SECS);
        self.run_bounded(OUTPUT_CAPTURE_LIMIT_BYTES, timeout)
    }

    /// Spawn the child under explicit bounds and wait for its typed exit.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] on spawn failure, output-limit
    /// breach, reader failure, or timeout.
    pub fn run_bounded(&self, cap: usize, timeout: Duration) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(cap, timeout, &CancelHandle::new())
    }

    /// Spawn the child under explicit bounds plus external cancellation.
    /// Residual: grandchildren inheriting the pipes can delay EOF after kill.
    ///
    /// A pre-cancelled handle fails without spawning; mid-run
    /// cancellation kills the child. Both surface as typed
    /// [`MiseError::SpawnFailed`], classified by [`is_cancel_or_timeout`].
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] on spawn failure, output-limit
    /// breach, reader failure, timeout, or cancellation.
    pub fn run_cancellable(
        &self,
        cap: usize,
        timeout: Duration,
        cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        let program = self.program.to_string_lossy().into_owned();
        let fail = |message: &str| MiseError::SpawnFailed {
            program: program.clone(),
            message: message.to_owned(),
        };
        if cancel.is_cancelled() {
            return Err(fail(SPAWN_CANCELLED_MESSAGE));
        }
        let mut child = self
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| fail(&err.to_string()))?;
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let out_reader = std::thread::spawn(move || read_capped(stdout, cap));
        let err_reader = std::thread::spawn(move || read_capped(stderr, cap));
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.is_cancelled() {
                drop((child.kill(), child.wait(), out_reader, err_reader));
                return Err(fail(SPAWN_CANCELLED_MESSAGE));
            }
            let status = child.try_wait().map_err(|err| fail(&err.to_string()))?;
            if let Some(status) = status {
                let (out, out_capped) = out_reader
                    .join()
                    .map_err(|_| fail("reader_panicked:stdout"))?;
                let (err, err_capped) = err_reader
                    .join()
                    .map_err(|_| fail("reader_panicked:stderr"))?;
                if out_capped || err_capped {
                    let stream = if out_capped { "stdout" } else { "stderr" };
                    return Err(fail(&format!("{stream}_limit_exceeded:{cap}")));
                }
                let code = status.code();
                let success = status.success();
                return Ok(ProcessOutput {
                    stdout: out,
                    stderr: err,
                    code,
                    signal: signal_of(status),
                    success,
                });
            }
            if Instant::now() >= deadline {
                drop((child.kill(), child.wait(), out_reader, err_reader));
                let message = format!("{SPAWN_TIMEOUT_MESSAGE_PREFIX}{}", timeout.as_secs());
                return Err(fail(&message));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if self.policy == EnvPolicy::RepoTask {
            command.env_clear();
        }
        for (key, value) in self.full_env() {
            command.env(key, value);
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command.stdin(Stdio::null());
        command
    }

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
                        program: "mise".to_owned(),
                    });
                }
                mise_argv_tail(subcommand, tool_specs, payload)
            }
            None => mise_install_argv_tail(tool_specs),
        };
        let (extra_env, policy) = if disable_install {
            (pairs_of(&NO_AUTO_INSTALL_ENV), EnvPolicy::Verify)
        } else {
            (Vec::new(), EnvPolicy::Bootstrap)
        };
        Ok(Self {
            program: OsString::from("mise"),
            args,
            cwd: None,
            extra_env,
            policy,
        })
    }
}

pub(crate) fn mise_argv_tail(sub: &str, specs: &[String], payload: &[OsString]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(specs.len() + payload.len() + 6);
    args.extend(MISE_GLOBAL_FLAGS.iter().map(OsString::from));
    args.push(OsString::from(sub));
    args.extend(specs.iter().map(OsString::from));
    args.push(OsString::from(TOOL_COMMAND_SEPARATOR));
    args.extend(payload.iter().cloned());
    args
}

pub(crate) fn mise_install_argv_tail(specs: &[String]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(specs.len() + 4);
    args.extend(MISE_GLOBAL_FLAGS.iter().map(OsString::from));
    args.push(OsString::from("install"));
    args.extend(specs.iter().map(OsString::from));
    args
}
