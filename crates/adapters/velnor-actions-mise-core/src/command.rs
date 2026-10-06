//! Fixed subprocess wrapper: the sole `std::process::Command` constructor.
//! Policy (`env`) and output (`output`) declare here.
mod cancellable;
mod check;
mod env;
mod output;
mod qualified_acquisition;
mod tofu;
pub use check::resolve_program as resolve_check_program;

pub use self::env::{
    CREDENTIAL_ALLOWLIST_BASELINE, CREDENTIAL_ALLOWLIST_BOOTSTRAP, CREDENTIAL_ENV_KEYS,
    ENDPOINT_ENV_KEYS, EnvPolicy, ISOLATION_ENV, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV,
    NO_AUTO_INSTALL_ENV, PROXY_ENV_KEYS, RUSTUP_TOOLCHAIN_ENV, TF_CLI_CONFIG_FILE_ENV,
    TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF,
    TF_PLUGIN_CACHE_DIR_ENV, is_denied_credential_key, is_denied_endpoint_key, is_reserved_env_key,
    proxy_passthrough, toolchain_env,
};
use self::env::{pairs_of, redact_env_for_debug, strip_credentials};
pub use self::output::redact_argv_for_debug;
pub use self::output::{
    CancelHandle, ProcessOutput, SPAWN_CANCELLED_MESSAGE, SPAWN_DEADLINE_MESSAGE,
    SPAWN_TIMEOUT_MESSAGE_PREFIX, is_cancel_or_timeout,
};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::MiseError;

/// Mise global flags, always placed before the subcommand.
///
/// Every mise invocation carries all three, installs included: no mise
/// command ever loads repo config, env files, or hooks. Explicit
/// `tool@exact` specs are the sole version authority.
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

/// Fully isolated, shell-free child invocation (`Debug` redacts secrets).
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

    /// Direct program invocation without the mise wrapper.
    #[must_use]
    pub fn direct(program: &str, args: Vec<OsString>) -> Self {
        Self {
            program: OsString::from(program),
            args,
            cwd: None,
            extra_env: Vec::new(),
            policy: EnvPolicy::Discovery,
        }
    }

    /// Build a repo-task child: cleared env plus declared inputs.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] on a reserved declared key.
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

    /// Append extras; reserved keys fail loud naming the key.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] on the first reserved key.
    pub fn with_env(mut self, extra: &[(OsString, OsString)]) -> Result<Self, MiseError> {
        for pair in extra {
            let key = pair.0.to_string_lossy();
            if is_reserved_env_key(&key) {
                return Err(MiseError::InvalidStepInput {
                    field: key.into_owned(),
                    value: "reserved_env_key".to_owned(),
                });
            }
            self.extra_env.push(pair.clone());
        }
        Ok(self)
    }

    /// Reassign the typed env policy (crate-internal constructors only).
    ///
    /// The only caller is the pinned-exec constructor, which selects
    /// [`EnvPolicy::Baseline`] for the `gh` baseline-lookup tool; every
    /// other command keeps the policy its own constructor assigned.
    #[must_use]
    pub fn with_policy(mut self, policy: EnvPolicy) -> Self {
        self.policy = policy;
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
    ///
    /// Qualified probes receive explicit extras only. Qualified checks
    /// retain task config visibility; other policies keep the fixed overlay.
    #[must_use]
    pub fn full_env(&self) -> Vec<(OsString, OsString)> {
        let mut env = if self.policy == EnvPolicy::QualifiedProbe {
            Vec::new()
        } else {
            Self::env_overlay()
        };
        if self.policy == EnvPolicy::QualifiedCheck {
            env.retain(|(key, _)| key != "MISE_NO_CONFIG");
        }
        env.extend(self.extra_env.iter().cloned());
        env
    }

    /// Full child env over a parent snapshot (pure [`Self::run`] contract).
    #[must_use]
    pub fn spawn_env(&self, parent: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
        self.policy.child_env(parent, &self.full_env())
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
    /// # Errors
    /// Returns [`MiseError::SpawnFailed`] on spawn/output/timeout failure.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        let timeout = Duration::from_secs(RUN_TIMEOUT_SECS);
        self.run_bounded(OUTPUT_CAPTURE_LIMIT_BYTES, timeout)
    }

    /// Spawn the child under explicit bounds and wait for its typed exit.
    /// # Errors
    /// Returns [`MiseError::SpawnFailed`] on spawn/output/timeout failure.
    pub fn run_bounded(&self, cap: usize, timeout: Duration) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(cap, timeout, &CancelHandle::new())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if self.policy.inherits_parent() {
            strip_credentials(&mut command, self.policy);
        } else {
            command.env_clear();
            if self.policy.allows_proxy_passthrough() {
                let parent: Vec<(OsString, OsString)> = std::env::vars_os().collect();
                command.envs(proxy_passthrough(&parent));
            }
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

/// Global flags plus `sub`, tool specs, separator, and payload.
#[must_use]
pub fn mise_argv_tail(sub: &str, specs: &[String], payload: &[OsString]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(specs.len() + payload.len() + 6);
    args.extend(MISE_GLOBAL_FLAGS.iter().map(OsString::from));
    args.push(OsString::from(sub));
    args.extend(specs.iter().map(OsString::from));
    args.push(OsString::from(TOOL_COMMAND_SEPARATOR));
    args.extend(payload.iter().cloned());
    args
}

/// Global flags plus `install` and tool specs.
#[must_use]
pub fn mise_install_argv_tail(specs: &[String]) -> Vec<OsString> {
    let mut args = Vec::with_capacity(specs.len() + 4);
    args.extend(MISE_GLOBAL_FLAGS.iter().map(OsString::from));
    args.push(OsString::from("install"));
    args.extend(specs.iter().map(OsString::from));
    args
}
