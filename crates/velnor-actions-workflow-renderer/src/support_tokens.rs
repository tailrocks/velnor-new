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

/// Shell steps allowed ambient auth, by step name.
///
/// The renderer must not depend on the Mise adapter, so externally
/// owned names mirror as literals; end-to-end generation tests render
/// the real steps through this gate, so a drifted literal fails there,
/// not here. Fetch and prepare download tools and sources (registry
/// auth is their purpose); nested fetch names carry a manifest suffix.
/// The pinned offline analyzers (deny, machete, zizmor, actionlint,
/// plus the config-selected verification steps) execute no repository
/// code and cold-install their tools, so they run ambient: scrubbing
/// broke `ubi:` installs (API 401) and zizmor (empty-token abort), CI
/// run 36815180228. Plan and fetch-reports are internal steps with no
/// shell env to gate, and release publishes through `gh` (allowlisted
/// by job ID below).
const AMBIENT_AUTH_STEPS: [&str; 12] = [
    "Prepare pinned tools",
    "Prepare Rust components",
    "Fetch Cargo sources",
    crate::steps::DENY_STEP_NAME,
    crate::steps::MACHETE_STEP_NAME,
    "Run zizmor",
    "Run actionlint",
    "Run markdownlint",
    "Check strict JSON",
    "Check frontmatter IDs",
    "Check links",
    "Run native validators",
];

/// True when a step name carries ambient-auth permission.
///
/// Exact match, except genuine generator nested-fetch names, which
/// share the root fetch purpose.
fn is_ambient_auth_step(name: &str) -> bool {
    AMBIENT_AUTH_STEPS.contains(&name) || is_generator_nested_fetch(name)
}

/// Prefix of generator nested-fetch names (`Fetch Cargo sources (<root>/Cargo.toml)`).
const FETCH_SOURCES_PREFIX: &str = "Fetch Cargo sources (";

/// True for genuine generator nested-fetch names only.
///
/// The generator emits `Fetch Cargo sources (<root>/Cargo.toml)` for
/// roots passing [`velnor_actions_contract::validate_fetch_root`]; the
/// exemption requires the exact shape — prefix, `/Cargo.toml)`
/// suffix, and a validated root — so a crafted lookalike (unclosed
/// paren, trailing text, `..`, `$`, quotes) never inherits ambient
/// auth through a bare prefix match.
fn is_generator_nested_fetch(name: &str) -> bool {
    let Some(inner) = name
        .strip_prefix(FETCH_SOURCES_PREFIX)
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    let Some(root) = inner.strip_suffix("/Cargo.toml") else {
        return false;
    };
    !root.is_empty() && velnor_actions_contract::validate_fetch_root(root).is_ok()
}

/// Reject credential leaks in one step's env, argv, and action inputs.
///
/// Active leaks report before missing protection: a step that prints a
/// token fails on the print even when it also lacks the scrub overlay.
fn check_step_tokens(id: &str, step: &Step) -> Result<(), RenderError> {
    match &step.kind {
        StepKind::Shell { run, env } => {
            check_env_tokens(id, env)?;
            for arg in strip_unset_argv(run) {
                if names_token(&strip_unset_prelude(arg)) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_run:{id}:{}",
                        step.name
                    )));
                }
            }
            check_scrub_coverage(id, &step.name, env)?;
        }
        StepKind::Action { with, env, .. } => {
            for value in with.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_input:{id}:{}",
                        step.name
                    )));
                }
            }
            for value in env.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_env:{id}:{}",
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
    if id == super::RELEASE_JOB_ID || is_ambient_auth_step(name) {
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
    &run[crate::toolchain_env::unset_prefix_len(run)..]
}

/// Script with the exact credential-unset prelude stripped for scanning.
///
/// The first prelude occurrence forming a shell command unit strips:
/// at the start or after a separator (`&&`, `;`, newline, `{`, `(`),
/// followed by a separator or the end. Only the exact fixed text
/// strips, and only once: a script merely starting with `unset`,
/// quoting the prelude (an `echo` of token names is still exfil),
/// embedding token names elsewhere, or doubling the prelude
/// (caller/render drift) still scans whole and trips fail-closed.
fn strip_unset_prelude(script: &str) -> String {
    use crate::toolchain_env::credential_unset_prelude;
    let prelude = credential_unset_prelude();
    for (at, _) in script.match_indices(prelude.as_str()) {
        if is_prelude_command_unit(&script[..at], &script[at + prelude.len()..]) {
            let mut stripped = String::with_capacity(script.len() - prelude.len());
            stripped.push_str(&script[..at]);
            stripped.push_str(&script[at + prelude.len()..]);
            return stripped;
        }
    }
    script.to_owned()
}

/// True when a prelude occurrence sits in command position.
///
/// The left edge must be the script start or follow a separator; the
/// right edge must be the end or lead a separator. A quoted or
/// suffixed occurrence (an `echo` of the text) is not a command.
fn is_prelude_command_unit(before: &str, after: &str) -> bool {
    const LEFT: [&str; 6] = ["&& ", "; ", "{ ", "\n", "( ", "("];
    const RIGHT: [char; 8] = [' ', ';', '&', '\n', '|', ')', '}', '\t'];
    (before.is_empty() || LEFT.iter().any(|sep| before.ends_with(sep)))
        && (after.is_empty() || after.starts_with(RIGHT))
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
