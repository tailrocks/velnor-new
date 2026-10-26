// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "command_cancellable.rs"]
mod cancellable;
#[path = "check_command.rs"]
mod check;
#[path = "command_credential.rs"]
mod credential;
#[path = "command_env.rs"]
mod env;
#[path = "command_git.rs"]
mod git;
#[path = "command_output.rs"]
mod output;
#[path = "command_private_fetch.rs"]
mod private_fetch;
#[cfg(unix)]
#[path = "command_process_group.rs"]
mod process_group;
#[path = "command_progress.rs"]
mod progress;
#[path = "qualified_acquisition.rs"]
mod qualified_acquisition;
#[path = "command_release_signer.rs"]
mod release_signer;
#[path = "command_stream.rs"]
mod stream;
#[path = "command_tofu.rs"]
mod tofu;
pub use check::resolve_program as resolve_check_program;
pub use credential::is_safe_credential_value;
pub use private_fetch::{
    CREDENTIAL_ALLOWLIST_PRIVATE_FETCH, FETCH_REDIRECT_ENV_KEYS, PrivateFetchCredential,
    is_fetch_redirect_key,
};

pub use self::env::{
    CREDENTIAL_ALLOWLIST_BOOTSTRAP, CREDENTIAL_ALLOWLIST_GH_API, CREDENTIAL_ENV_KEYS,
    ENDPOINT_ENV_KEYS, EnvPolicy, ISOLATION_ENV, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV,
    NO_AUTO_INSTALL_ENV, PROXY_ENV_KEYS, RUSTUP_TOOLCHAIN_ENV, TF_CLI_CONFIG_FILE_ENV,
    TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF,
    TF_PLUGIN_CACHE_DIR_ENV, is_denied_credential_key, is_denied_endpoint_key, is_reserved_env_key,
    proxy_passthrough, toolchain_env,
};
use self::env::{pairs_of, redact_env_for_debug, strip_credentials};
pub(crate) use self::output::redact_argv_for_debug;
pub use self::output::{
    CancelHandle, ProcessFileOutput, ProcessGroupDrainReceipt, ProcessOutput,
    SPAWN_CANCELLED_MESSAGE, SPAWN_DEADLINE_MESSAGE, SPAWN_TIMEOUT_MESSAGE_PREFIX,
    is_cancel_or_timeout, is_cleanup_failure,
};
pub use crate::compiler_observer::{
    MBX_PREVIOUS_RUSTC_WRAPPER_ENV, MBX_TARGET_VIEWS_DISABLED, MBX_TARGET_VIEWS_ENV,
    RUSTC_WRAPPER_ENV, RUSTDOC_ENV,
};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::MiseError;

fn cleared_parent_passthrough(
    policy: EnvPolicy,
    parent: &[(OsString, OsString)],
) -> Vec<(OsString, OsString)> {
    match policy {
        EnvPolicy::GitHubToken => {
            let mut env: Vec<_> = parent
                .iter()
                .filter(|(key, _)| key == "PATH" || key == "HOME")
                .cloned()
                .collect();
            let last_env = |name: &str| {
                parent
                    .iter()
                    .rfind(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
            };
            let data_dir = last_env("MISE_DATA_DIR")
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    last_env("XDG_DATA_HOME")
                        .filter(|value| !value.is_empty())
                        .map(|value| PathBuf::from(value).join("mise").into_os_string())
                });
            env.extend(data_dir.map(|value| (OsString::from("MISE_DATA_DIR"), value)));
            env
        }
        policy if policy.allows_proxy_passthrough() => proxy_passthrough(parent),
        _ => Vec::new(),
    }
}

pub const MISE_GLOBAL_FLAGS: [&str; 3] = ["--no-config", "--no-env", "--no-hooks"];

pub const TOOL_COMMAND_SEPARATOR: &str = "--";

pub const ALLOWED_MISE_SUBCOMMANDS: [&str; 3] = ["exec", "install", "run"];

pub const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 8 * 1024 * 1024;

pub const RUN_TIMEOUT_SECS: u64 = 600;

#[must_use]
pub fn is_allowed_mise_subcommand(subcommand: &str) -> bool {
    ALLOWED_MISE_SUBCOMMANDS.contains(&subcommand)
}

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
    pub fn mise_exec(tool_specs: &[String], payload: &[OsString]) -> Result<Self, MiseError> {
        Self::mise_with_subcommand("exec", tool_specs, Some(payload), true)
    }

    pub fn mise_install(tool_specs: &[String]) -> Result<Self, MiseError> {
        if tool_specs.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Self::mise_with_subcommand("install", tool_specs, None, false)
    }

    pub(crate) fn mise_ls_json(tool: &str) -> Self {
        let mut args: Vec<OsString> = MISE_GLOBAL_FLAGS.iter().map(OsString::from).collect();
        args.push(OsString::from("ls"));
        args.push(OsString::from("--json"));
        args.push(OsString::from(tool));
        Self {
            program: OsString::from("mise"),
            args,
            cwd: None,
            extra_env: pairs_of(&NO_AUTO_INSTALL_ENV),
            policy: EnvPolicy::Verify,
        }
    }

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

    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn with_env(mut self, extra: &[(OsString, OsString)]) -> Result<Self, MiseError> {
        for pair in extra {
            let key = pair.0.to_string_lossy();
            if is_reserved_env_key(&key)
                || (self.policy == EnvPolicy::Discovery
                    && self.program == "git"
                    && key == "GIT_OPTIONAL_LOCKS")
            {
                return Err(MiseError::InvalidStepInput {
                    field: key.into_owned(),
                    value: "reserved_env_key".to_owned(),
                });
            }
            self.extra_env.push(pair.clone());
        }
        Ok(self)
    }

    pub(crate) fn with_policy(mut self, policy: EnvPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_compiler_observer(
        self,
        observer: &str,
        rustdoc_observer: &str,
        previous_rustc_wrapper: Option<&str>,
        previous_rustdoc: Option<&str>,
    ) -> Result<Self, MiseError> {
        let overlay = crate::compiler_observer::observer_env(
            observer,
            rustdoc_observer,
            previous_rustc_wrapper,
            previous_rustdoc,
        )?;
        self.with_env(&overlay)
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.clone());
        argv.extend(self.args.iter().cloned());
        argv
    }

    #[must_use]
    pub fn env_overlay() -> Vec<(OsString, OsString)> {
        pairs_of(&ISOLATION_ENV)
    }

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

    #[must_use]
    pub fn spawn_env(&self, parent: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
        self.policy.child_env(parent, &self.full_env())
    }

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

    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        let timeout = Duration::from_secs(RUN_TIMEOUT_SECS);
        self.run_bounded(OUTPUT_CAPTURE_LIMIT_BYTES, timeout)
    }

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
            let parent: Vec<(OsString, OsString)> = std::env::vars_os().collect();
            command.envs(cleared_parent_passthrough(self.policy, &parent));
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
