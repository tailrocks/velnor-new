//! Fixed subprocess wrapper: the sole `std::process::Command` constructor.
//! Trusted tooling inherits the parent env; repo-task children spawn cleared.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::MiseError;

/// Isolation environment applied to every spawned process.
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

/// Mise subcommands Velnor may emit: `exec`, `install`, and `run`.
pub const ALLOWED_MISE_SUBCOMMANDS: [&str; 3] = ["exec", "install", "run"];

/// Environment disabling implicit tool installation for verification runs.
pub const NO_AUTO_INSTALL_ENV: [(&str, &str); 2] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
];

/// Captured bytes kept per stream; past this the run fails closed.
pub const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// Default run deadline in seconds; past this the child is killed.
pub const RUN_TIMEOUT_SECS: u64 = 600;

/// Environment name for the Velnor-owned mise Rust home.
pub const MISE_RUSTUP_HOME_ENV: &str = "MISE_RUSTUP_HOME";

/// Environment name for the Velnor-owned mise Cargo home.
pub const MISE_CARGO_HOME_ENV: &str = "MISE_CARGO_HOME";

/// Environment name selecting the exact Rust toolchain for Cargo runs.
pub const RUSTUP_TOOLCHAIN_ENV: &str = "RUSTUP_TOOLCHAIN";

/// Whether a subcommand is inside the Velnor mise allowlist.
#[must_use]
pub fn is_allowed_mise_subcommand(subcommand: &str) -> bool {
    ALLOWED_MISE_SUBCOMMANDS.contains(&subcommand)
}

/// Whether a key is reserved: isolation, install disable, or credentials.
/// Credentials are `MISE_GITHUB_TOKEN` plus the `GITHUB_TOKEN`/`GH_TOKEN`
/// aliases and `ACTIONS_RUNTIME_TOKEN`.
#[must_use]
pub fn is_reserved_env_key(key: &str) -> bool {
    ISOLATION_ENV.iter().any(|(own, _)| *own == key)
        || NO_AUTO_INSTALL_ENV.iter().any(|(own, _)| *own == key)
        || [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
        ]
        .contains(&key)
}

/// Typed child-process result: captured streams plus a typed exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Captured standard output bytes (bounded by the spawn cap).
    pub stdout: Vec<u8>,
    /// Captured standard error bytes (bounded by the spawn cap).
    pub stderr: Vec<u8>,
    /// Exit code when the child exited normally.
    pub code: Option<i32>,
    /// Terminating signal number when killed by a signal (Unix only).
    pub signal: Option<i32>,
    /// Whether the exit status reports success.
    pub success: bool,
}

impl ProcessOutput {
    /// Fail with [`MiseError::NonZeroExit`] unless the status reports success.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::NonZeroExit`] when the status reports failure.
    pub fn require_success(&self, program: &str) -> Result<&Self, MiseError> {
        if self.success {
            return Ok(self);
        }
        Err(MiseError::NonZeroExit {
            program: program.to_owned(),
            code: self.code,
            stderr: String::from_utf8_lossy(&self.stderr).into_owned(),
        })
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

/// Which parent environment a child may see: bootstrap, verify, and
/// discovery inherit; repo-task spawns from `env_clear` plus an explicit list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPolicy {
    /// Trusted tool download: inherits parent env plus isolation overlay.
    Bootstrap,
    /// Trusted evidence validation: inherits parent env plus isolation overlay.
    Verify,
    /// Read-only discovery probes: inherits parent env plus isolation overlay.
    Discovery,
    /// Repository task execution: cleared env plus declared inputs only.
    RepoTask,
}

/// A fully isolated, shell-free child-process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedCommand {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    extra_env: Vec<(OsString, OsString)>,
    policy: EnvPolicy,
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
    /// Residual: grandchildren inheriting the pipes can delay EOF after kill.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] on spawn failure, output-limit
    /// breach, reader failure, or timeout.
    pub fn run_bounded(&self, cap: usize, timeout: Duration) -> Result<ProcessOutput, MiseError> {
        let program = self.program.to_string_lossy().into_owned();
        let fail = |message: &str| MiseError::SpawnFailed {
            program: program.clone(),
            message: message.to_owned(),
        };
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
                return Err(fail(&format!("timeout_after_secs:{}", timeout.as_secs())));
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

/// Velnor-owned toolchain environment for one Cargo invocation.
#[must_use]
pub fn toolchain_env(rustup: &str, cargo: &str, toolchain: &str) -> Vec<(OsString, OsString)> {
    [
        (MISE_RUSTUP_HOME_ENV, rustup),
        (MISE_CARGO_HOME_ENV, cargo),
        (RUSTUP_TOOLCHAIN_ENV, toolchain),
    ]
    .iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

fn pairs_of<const N: usize>(table: &[(&str, &str); N]) -> Vec<(OsString, OsString)> {
    table
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

fn read_capped<R: std::io::Read>(pipe: Option<R>, limit: usize) -> (Vec<u8>, bool) {
    let Some(pipe) = pipe else {
        return (Vec::new(), false);
    };
    let mut buf = Vec::new();
    let capped = pipe
        .take(limit.saturating_add(1).try_into().unwrap_or(u64::MAX))
        .read_to_end(&mut buf)
        .is_err()
        || buf.len() > limit;
    (buf, capped)
}

fn signal_of(status: std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(&status)
    }
    #[cfg(not(unix))]
    {
        None
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
