//! Toolchain-home env for rendered Cargo steps (RQ-3.5, TASK-6.1).
//!
//! Every rendered Cargo step carries the owned-homes triple; values
//! arrive from the orchestrator (which reads Mise `ToolHomes`), never
//! invented here. This crate must not depend on the Mise adapter.

use std::collections::BTreeMap;

use crate::RenderError;

/// Toolchain-home env keys carried by every Cargo step.
pub const TOOLCHAIN_HOME_KEYS: [&str; 3] =
    ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"];

/// Credential keys forbidden in any rendered step env.
///
/// The Mise GitHub token, its `GITHUB_TOKEN`/`GH_TOKEN` aliases, the
/// runner token, the OIDC token-request pair, the registry token, and
/// the npm auth pair. Mirrors the Mise adapter's strip set
/// element-for-element without depending on it; the orchestrator pins
/// the two lists equal by test.
pub const STEP_CREDENTIAL_DENYLIST: [&str; 9] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "CARGO_REGISTRY_TOKEN",
    "NPM_TOKEN",
    "NODE_AUTH_TOKEN",
];

/// Endpoint-selector keys forbidden in any rendered step env.
///
/// `GH_HOST` would reroute `gh` — and any kept token — to an
/// attacker host; `GH_CONFIG_DIR` would load attacker-controlled
/// auth. Mirrors the Mise adapter's endpoint strip set
/// element-for-element without depending on it; the orchestrator pins
/// the two lists equal by test.
pub const STEP_ENDPOINT_DENYLIST: [&str; 2] = ["GH_HOST", "GH_CONFIG_DIR"];

/// True for a credential-shaped env key: the denylisted nine, any
/// `CARGO_REGISTRIES_*` entry (per-registry tokens the fixed list can
/// never enumerate), or any `*_TOKEN` name. Rejection is fail-closed:
/// an unrecognized credential-shaped key in a rendered map errors,
/// never passes through. Ambient keys the generator never emits stay
/// unreachable: the generator owns the whole workflow file, and the
/// freshness gate rejects hand edits carrying them.
#[must_use]
pub fn is_denied_credential_key(key: &str) -> bool {
    STEP_CREDENTIAL_DENYLIST.contains(&key)
        || key.starts_with("CARGO_REGISTRIES_")
        || key.ends_with("_TOKEN")
}

/// True for an endpoint-selector key: never emitted into rendered
/// step env.
#[must_use]
pub fn is_denied_endpoint_key(key: &str) -> bool {
    STEP_ENDPOINT_DENYLIST.contains(&key)
}

/// TF-family env prefixes no project task may declare.
///
/// `TF_*` steers `IaC` tool behavior (`TF_VAR_*` injects variables,
/// `TF_CLI_ARGS_*` appends flags, `TF_DATA_DIR`/`TF_CLI_CONFIG_FILE`
/// relocate state/config) and `CHECKPOINT_*` toggles phone-home
/// telemetry; untrusted project-task declarations must never smuggle
/// them in. Mirrors the Mise adapter's reserved rule for these
/// families without depending on it; the orchestrator pins the two
/// predicates equal by test on a corpus.
pub const STEP_TF_DENYLIST_PREFIXES: [&str; 2] = ["TF_", "CHECKPOINT_"];

/// TF-family keys project tasks may still declare: the harmless
/// automation pair, which the generator itself threads everywhere.
pub const STEP_TF_ALLOWLIST: [&str; 2] = ["TF_IN_AUTOMATION", "TF_INPUT"];

/// True for a TF-family env key no project task may declare: the
/// `TF_`/`CHECKPOINT_` prefixes minus the automation-pair allowlist.
///
/// (One future reserved family stays Mise-only: no tool reads it
/// and it has no wire presence, so the renderer leaves it alone; the
/// orchestrator parity test pins the deliberate divergence.)
#[must_use]
pub fn is_denied_tf_key(key: &str) -> bool {
    if STEP_TF_ALLOWLIST.contains(&key) {
        return false;
    }
    STEP_TF_DENYLIST_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

/// Merge the toolchain-home triple into a step env map.
#[must_use]
pub fn with_toolchain_homes(
    env: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> BTreeMap<String, String> {
    let mut merged = env.clone();
    for (key, value) in [
        (TOOLCHAIN_HOME_KEYS[0], rustup_home),
        (TOOLCHAIN_HOME_KEYS[1], cargo_home),
        (TOOLCHAIN_HOME_KEYS[2], toolchain),
    ] {
        merged.insert(key.to_owned(), value.to_owned());
    }
    merged
}

/// Require the toolchain-home triple in a Cargo step env.
/// # Errors
pub fn check_toolchain_homes(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in TOOLCHAIN_HOME_KEYS {
        if env.get(key).is_none_or(String::is_empty) {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    Ok(())
}

/// Reject denied credential and endpoint keys in a step env map.
///
/// Pattern-matched, not list-matched: any credential-shaped key fails,
/// including registry and enterprise variants the fixed list omits.
/// # Errors
pub fn reject_denied_step_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in env.keys() {
        if is_denied_credential_key(key) {
            return Err(RenderError::BadCommand(format!(
                "credential_step_env:{key}"
            )));
        }
        if is_denied_endpoint_key(key) {
            return Err(RenderError::BadCommand(format!("endpoint_step_env:{key}")));
        }
    }
    Ok(())
}

/// Explicit empty credential/endpoint values stopping ambient inheritance.
///
/// Workflow- and job-scope definitions inherit into every step unless
/// a step key shadows them; these empty values are that shadow for the
/// nine credential keys plus the two endpoint keys. Runner-injected
/// `GITHUB_TOKEN` and the OIDC pair are NOT shadowed (the runner
/// overwrites step env after the workflow merge; see the D3 evidence
/// note on [`CREDENTIAL_UNSET_VARS`]), so steps executing repository
/// code pair this overlay with the unset wrapper. Empty is the only
/// legal scrub value: the render gate rejects any nonempty credential
/// as a leak.
#[must_use]
pub fn credential_scrub() -> BTreeMap<String, String> {
    STEP_CREDENTIAL_DENYLIST
        .iter()
        .chain(STEP_ENDPOINT_DENYLIST.iter())
        .map(|key| ((*key).to_owned(), String::new()))
        .collect()
}

/// Overlay the credential scrub onto a validated step env.
///
/// Callers validate the base first ([`checked_task_env`] rejects any
/// caller-supplied credential, empty or not); the overlay then blanks
/// all eleven keys by construction, never from caller input.
#[must_use]
pub fn with_credential_scrub(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut scrubbed = env.clone();
    scrubbed.extend(credential_scrub());
    scrubbed
}

/// Credentials no rendered step may observe, even as empty strings (D3).
///
/// Two independent reasons force true removal (`env -u` / `unset`) over
/// the empty-string scrub overlay. First, runner-controlled credentials
/// cannot be shadowed: evidence (`actions/runner`
/// `Runner.Worker/Handlers/ScriptHandler.cs` `RunAsync`, current
/// `main`) shows the handler loads the workflow step env into
/// `Environment`, then unconditionally overwrites every
/// runtime-context variable (`Environment[env.Key] = env.Value`,
/// covering `GITHUB_*`) plus `ACTIONS_ID_TOKEN_REQUEST_URL` and
/// `ACTIONS_ID_TOKEN_REQUEST_TOKEN` from the system connection before
/// spawning the step. The variables reference agrees: assignments to
/// `GITHUB_*`/`RUNNER_*` defaults are ignored. `ACTIONS_RUNTIME_TOKEN`
/// was not observed in the `run:`-step injection path (it is served to
/// action handlers, not inline scripts); it stays listed defensively
/// since unsetting an absent variable is a no-op.
///
/// Second, empty is poison for GitHub-auth consumers: CI run
/// 36815180228 proved `MISE_GITHUB_TOKEN=""` makes Mise send an empty
/// bearer (GitHub API 401, breaking `ubi:` tool installs) and
/// `GH_TOKEN=""` makes zizmor abort (`GitHub token cannot be empty`),
/// while truly-absent variables fall back to clean unauthenticated
/// fetches. `GH_HOST`/`GH_CONFIG_DIR` join the set for the same
/// reason: an empty endpoint selector misroutes or breaks `gh`-family
/// tools instead of selecting the default host. Absence also matches
/// the Mise adapter, whose local spawns strip these keys outright.
pub const CREDENTIAL_UNSET_VARS: [&str; 8] = [
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "ACTIONS_RUNTIME_TOKEN",
    "GITHUB_TOKEN",
    "MISE_GITHUB_TOKEN",
    "GH_TOKEN",
    "GH_HOST",
    "GH_CONFIG_DIR",
];

/// Shell prelude unsetting the unshadowable credentials.
///
/// Prefixes `sh -c` scripts that execute repository code, before any
/// repo-controlled byte runs. Each name is a fixed `A-Z_` literal, so
/// the prelude embeds injection-free into any fixed script.
#[must_use]
pub fn credential_unset_prelude() -> String {
    format!("unset {};", CREDENTIAL_UNSET_VARS.join(" "))
}

/// Prefix one `sh -c` script with the credential-unset prelude.
#[must_use]
pub fn with_credential_unset_script(script: &str) -> String {
    format!("{} {script}", credential_unset_prelude())
}

/// Prefix one direct-exec argv with `env -u` for each unset key.
///
/// For fixed vectors that exec a tool directly (`mise run`, `mise
/// exec` builds): `env` removes every [`CREDENTIAL_UNSET_VARS`] entry,
/// then execs the original argv unchanged. `env -u` is POSIX 2018 and
/// works on GNU and BSD userlands.
#[must_use]
pub fn with_env_unset_argv(argv: &[String]) -> Vec<String> {
    let mut unset = Vec::with_capacity(argv.len() + 2 * CREDENTIAL_UNSET_VARS.len() + 1);
    unset.push("env".to_owned());
    for var in CREDENTIAL_UNSET_VARS {
        unset.push("-u".to_owned());
        unset.push(var.to_owned());
    }
    unset.extend(argv.iter().cloned());
    unset
}

/// Length of the exact `env -u` unset prefix heading argv, else zero.
///
/// The emitter ([`with_env_unset_argv`]) and every prefix consumer
/// (run-line joining, token scanning) agree on this shape: `env`
/// followed by `-u` pairs naming only [`CREDENTIAL_UNSET_VARS`]
/// members. A bare `env` with no pairs still strips its one word;
/// `-u` with a non-fixed name stops the prefix (payload, not wrapper).
#[must_use]
pub(crate) fn unset_prefix_len(argv: &[String]) -> usize {
    if argv.first().is_none_or(|arg| arg != "env") {
        return 0;
    }
    let mut len = 1;
    while argv.len() >= len + 2
        && argv[len] == "-u"
        && CREDENTIAL_UNSET_VARS.contains(&argv[len + 1].as_str())
    {
        len += 2;
    }
    len
}

/// Isolation keys forbidden in project-task step env (P07-7 hook escape).
///
/// The isolation quartet plus the install-disable pair. Trusted generated
/// steps carry these from the Mise adapter's single source; untrusted
/// project-task declarations must never smuggle them in to re-enable
/// hooks, config, or implicit installs. Mirrors the Mise adapter's
/// reserved set minus credentials (denied separately above) without
/// depending on it.
pub const STEP_ISOLATION_DENYLIST: [&str; 6] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
];

/// Static Mise environment variables applied to every workflow job.
pub const MISE_STATIC_ENV: [(&str, &str); 6] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
    ("MISE_LOCKFILE", "0"),
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
];

/// Centralized workflow-level environment variables hoisted from jobs and steps.
///
/// Contains the 11 blank credential scrub / endpoint variables stopping
/// ambient inheritance and the 6 static Mise isolation / auto-install variables.
#[must_use]
pub fn workflow_level_env() -> BTreeMap<String, String> {
    let mut map = credential_scrub();
    for (key, value) in MISE_STATIC_ENV {
        map.insert(key.to_owned(), value.to_owned());
    }
    map
}

/// Centralized job-level environment variables hoisted from steps.
#[must_use]
pub fn job_level_env() -> BTreeMap<String, String> {
    workflow_level_env()
}

/// Reject privileged isolation keys in a project-task step env map.
/// # Errors
pub fn reject_privileged_task_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in STEP_ISOLATION_DENYLIST {
        if env.contains_key(key) {
            return Err(RenderError::BadCommand(format!(
                "privileged_task_env:{key}"
            )));
        }
    }
    Ok(())
}

/// Reject TF-family keys in a project-task step env map.
///
/// Pattern-matched, not list-matched: any `TF_*`/`CHECKPOINT_*` key
/// fails except the automation-pair allowlist.
/// # Errors
pub fn reject_denied_tf_keys(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in env.keys() {
        if is_denied_tf_key(key) {
            return Err(RenderError::BadCommand(format!("tf_task_env:{key}")));
        }
    }
    Ok(())
}

/// Merge the triple over untrusted project-task declarations, refusing all privileged keys.
///
/// Same contract as [`checked_task_env`], plus isolation-key and
/// TF-family denial: a hostile project task cannot re-enable
/// hooks/config, steer `IaC` tool behavior, or smuggle credentials
/// through its declared env.
/// # Errors
pub fn checked_project_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    reject_privileged_task_keys(base)?;
    reject_denied_tf_keys(base)?;
    checked_task_env(base, rustup_home, cargo_home, toolchain)
}

/// Merge the triple over a validated base, refusing blanks and credentials.
///
/// The base carries the caller's policy pairs plus step extras; blank
/// triple inputs and denied credential keys anywhere in the merged map
/// fail closed before any step renders.
/// # Errors
pub fn checked_task_env(
    base: &BTreeMap<String, String>,
    rustup_home: &str,
    cargo_home: &str,
    toolchain: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    for (key, value) in [
        (TOOLCHAIN_HOME_KEYS[0], rustup_home),
        (TOOLCHAIN_HOME_KEYS[1], cargo_home),
        (TOOLCHAIN_HOME_KEYS[2], toolchain),
    ] {
        if value.is_empty() {
            return Err(RenderError::BadCommand(format!(
                "missing_toolchain_home:{key}"
            )));
        }
    }
    let merged = with_toolchain_homes(base, rustup_home, cargo_home, toolchain);
    check_toolchain_homes(&merged)?;
    reject_denied_step_keys(&merged)?;
    Ok(merged)
}
