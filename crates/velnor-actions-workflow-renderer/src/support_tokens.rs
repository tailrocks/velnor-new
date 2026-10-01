//! Token-hygiene gate for rendered jobs (split from `support`).
//!
//! Declared from `support.rs` (`#[path]`, no `lib.rs` edit).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
};

/// Token hygiene: `${{ github.token }}` only as plan/final `GH_TOKEN`.
///
/// Parallelism §5: the planning process receives the token only as
/// `GH_TOKEN`, never printed or inherited by task execution; before any
/// repository task starts, credential variables are stripped from the
/// child environment. The final job's report fetch needs the same
/// read-only token for exact `gh` artifact downloads. Enforced here
/// against the full nine-key credential set (single source:
/// [`crate::toolchain_env::STEP_CREDENTIAL_DENYLIST`], never a local
/// copy): any nonempty credential fails everywhere, `GH_TOKEN` carries
/// the exact `${{ github.token }}` value only in the plan/final jobs,
/// empty values are the explicit scrub overlay stopping ambient
/// inheritance, and no `run:` content or action input may name a token
/// (nothing prints or forwards one).
pub(crate) fn check_token_hygiene(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for step in &job.steps {
            check_step_tokens(id, step)?;
        }
    }
    Ok(())
}

/// Shell steps allowed ambient auth, by exact step name.
///
/// The renderer must not depend on the Mise adapter, so these mirror
/// the adapter's `PREPARE_*` names plus the orchestrator's fetch name
/// as literals; end-to-end generation tests render the real steps
/// through this gate, so a drifted literal fails there, not here.
/// Fetch and prepare download tools and sources (registry auth is
/// their purpose); plan and fetch-reports are internal steps with no
/// shell env to gate, and release publishes through `gh`
/// (allowlisted by job ID below).
const AMBIENT_AUTH_STEPS: [&str; 3] = [
    "Prepare pinned tools",
    "Prepare Rust components",
    "Fetch Cargo sources",
];

/// Reject credential leaks in one step's env, argv, and action inputs.
///
/// Active leaks report before missing protection: a step that prints a
/// token fails on the print even when it also lacks the scrub overlay.
fn check_step_tokens(id: &str, step: &Step) -> Result<(), RenderError> {
    match &step.kind {
        StepKind::Shell { run, env } => {
            check_env_tokens(id, env)?;
            for arg in strip_unset_argv(run) {
                if names_token(strip_unset_prelude(arg)) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_run:{id}:{}",
                        step.name
                    )));
                }
            }
            check_scrub_coverage(id, &step.name, env)?;
        }
        StepKind::Action { with, .. } => {
            for value in with.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_input:{id}:{}",
                        step.name
                    )));
                }
            }
        }
        StepKind::Internal { .. } => {}
    }
    Ok(())
}

/// Reject nonempty credentials everywhere; scope `GH_TOKEN` to plan/final jobs.
///
/// Empty values on denylisted keys are the explicit scrub overlay (see
/// [`crate::toolchain_env::credential_scrub`]) and pass; anything else
/// on those keys is a leak. `MISE_GITHUB_TOKEN` contains `GITHUB_TOKEN`
/// as a substring, so it needs no separate `names_token` arm.
/// True for the one scoped token binding: `GH_TOKEN` carrying exactly
/// `${{ github.token }}` in the plan or final job.
fn is_scoped_gh_token(id: &str, env: &BTreeMap<String, String>) -> bool {
    (id == PLAN_JOB_ID || id == FINAL_JOB_ID)
        && env
            .get("GH_TOKEN")
            .is_some_and(|value| value == "${{ github.token }}")
}

fn check_env_tokens(id: &str, env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    use crate::toolchain_env::is_denied_credential_key;
    for (key, value) in env {
        if is_denied_credential_key(key) {
            if value.is_empty() {
                continue;
            }
            if key == "GH_TOKEN" {
                if is_scoped_gh_token(id, env) {
                    continue;
                }
                return Err(RenderError::InvalidWorkflow(format!(
                    "token_misplaced:{id}:GH_TOKEN"
                )));
            }
            return Err(RenderError::InvalidWorkflow(format!(
                "credential_env:{id}:{key}"
            )));
        }
        if names_token(value) {
            return Err(RenderError::InvalidWorkflow(format!(
                "token_in_env:{id}:{key}"
            )));
        }
    }
    Ok(())
}

/// Require the full scrub overlay on every shell step outside the
/// ambient-auth allowlist.
///
/// Presence checks alone let an empty env pass and keep scrub opt-in
/// per constructor; this gate inverts the default to opt-out: scrubbed
/// or explicitly allowlisted, nothing else renders.
fn check_scrub_coverage(
    id: &str,
    name: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    use crate::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    if id == super::RELEASE_JOB_ID || AMBIENT_AUTH_STEPS.contains(&name) {
        return Ok(());
    }
    let scoped = is_scoped_gh_token(id, env);
    let scrubbed = STEP_CREDENTIAL_DENYLIST
        .iter()
        .all(|key| env.get(*key).is_some_and(String::is_empty) || (*key == "GH_TOKEN" && scoped));
    if scrubbed {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "missing_scrub:{id}:{name}"
        )))
    }
}

/// Argv with the `env -u` credential-unset prefix stripped for scanning.
///
/// The wrapper mentions token names to REMOVE them; scanning it would
/// flag the protection as exfil. Only exact fixed-var pairs strip:
/// anything else (including `-u` with a non-fixed name) still scans,
/// and the payload after the prefix always scans.
fn strip_unset_argv(run: &[String]) -> &[String] {
    use crate::toolchain_env::CREDENTIAL_UNSET_VARS;
    let mut rest = run;
    if rest.first().is_some_and(|arg| arg == "env") {
        rest = &rest[1..];
        while rest.len() >= 2
            && rest[0] == "-u"
            && CREDENTIAL_UNSET_VARS.contains(&rest[1].as_str())
        {
            rest = &rest[2..];
        }
    }
    rest
}

/// Script with the exact credential-unset prelude stripped for scanning.
///
/// Only the fixed prelude text strips; a script merely starting with
/// `unset` (or embedding token names elsewhere) still scans whole.
fn strip_unset_prelude(script: &str) -> &str {
    use crate::toolchain_env::credential_unset_prelude;
    script
        .strip_prefix(&credential_unset_prelude())
        .and_then(|rest| rest.strip_prefix(' '))
        .unwrap_or(script)
}

/// True when text names a token handle (never printed or forwarded).
///
/// The `*_TOKEN` arm subsumes every `*_TOKEN` denylisted name plus npm
/// and per-registry variants; the remaining arms cover the URL-shaped
/// OIDC handle, registry prefixes without a token suffix, and the
/// expression handles (`github.token`, secrets). The `secrets.` match
/// is case-insensitive and prefix-wide (X2): any casing or secret name
/// fails closed, not just `GITHUB_TOKEN`.
fn names_token(text: &str) -> bool {
    text.contains("_TOKEN")
        || text.contains("ACTIONS_ID_TOKEN_REQUEST_URL")
        || text.contains("CARGO_REGISTRIES_")
        || text.contains("github.token")
        || text.to_ascii_lowercase().contains("secrets.")
}
