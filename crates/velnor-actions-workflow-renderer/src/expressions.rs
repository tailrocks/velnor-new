//! GitHub expression allowlists for names, env, and action inputs.
//!
//! Constructors accept `${{ }}` (narrow gates diagnose misuse with
//! specific codes; there is no blanket argv ban, and the release
//! `secret_outside_bootstrap` gate keeps diagnosing placement). Step
//! display names never legitimately carry one, and env/action values
//! only ever carry fixed runner-provided expressions, so those layers
//! fail closed on anything unlisted here.

use crate::RenderError;

/// Reject `${{` plus control characters in a step display name.
///
/// Shared by shell, action, and internal constructors: names are fixed
/// literals, and an expression in one is injection or corruption.
/// # Errors
pub(crate) fn check_name_content(name: &str) -> Result<(), RenderError> {
    if name.contains("${{") {
        return Err(RenderError::BadCommand(format!(
            "expression_in_name:{name}"
        )));
    }
    if name
        .chars()
        .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(RenderError::BadCommand(format!("control_in_name:{name}")));
    }
    Ok(())
}

/// Extract `${{ ... }}` span inners; `None` on unclosed/nested/empty.
fn expression_spans(text: &str) -> Option<Vec<&str>> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("${{") {
        let after = &rest[at + 3..];
        let end = after.find("}}")?;
        let inner = after[..end].trim();
        if inner.is_empty() || inner.contains("${{") || inner.contains('\n') {
            return None;
        }
        spans.push(inner);
        rest = &after[end + 2..];
    }
    Some(spans)
}

/// Exact `${{ }}` inners permitted in shell-step env values.
///
/// Runner paths, the release tag, plan-matrix coordinates, fixed
/// workflow secret handles, and AWS action outputs. Notably absent: `github.token` (render-time fetch binding only)
/// and run IDs (never in env).
const ENV_EXPRESSIONS: [&str; 12] = [
    "runner.temp",
    "format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.event.pull_request.head.repo.fork",
    "github.ref_name",
    "github.event_name",
    "secrets.CARGO_REGISTRY_TOKEN",
    "secrets.GITHUB_TOKEN",
    "github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.sha",
    "inputs.cache_key",
    "steps.aws-credentials.outputs.aws-access-key-id",
    "steps.aws-credentials.outputs.aws-secret-access-key",
    "steps.aws-credentials.outputs.aws-session-token",
];

/// Exact `${{ }}` inners permitted in action `with:` values.
///
/// Run-scoped names, runner paths, matrix coordinates, the
/// push-gated cache-save flag, and runtime cache identities. Notably absent: every `secrets.*` handle (rejected
/// separately as `secret_in_action_input`).
const WITH_EXPRESSIONS: [&str; 13] = [
    "runner.temp",
    "github.run_id",
    "github.run_attempt",
    "env.VELNOR_RUN_KEY_ATTEMPT",
    "github.event_name == 'push'",
    "steps.v2.outputs.identity",
    "steps.v2.outputs.seed_admitted",
    "steps.tofu-providers.outputs.cache-key",
    "steps.tofu-providers.outputs.cache-path",
    "steps.cache-keys.outputs.sources_key",
    "steps.cache-keys.outputs.sources_restore_keys",
    "needs.plan.outputs.sources_key",
    "needs.plan.outputs.sources_restore_keys",
];

/// True for a Plan-published per-root provider key (`tofu_providers_key_<n>`).
fn is_tofu_providers_plan_key(inner: &str) -> bool {
    inner
        .strip_prefix("needs.plan.outputs.")
        .is_some_and(|name| {
            name.starts_with(
                velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_KEY_OUTPUT_PREFIX,
            ) && velnor_actions_contract::workflow::cache_family::is_plan_cache_output(name)
        })
}

/// True for a dependency seed key or restore prefix from the key step or Plan.
fn is_rsdeps_key_reference(inner: &str) -> bool {
    use velnor_actions_contract::workflow::cache_family::{
        RSDEPS_EXACT_OUTPUT_PREFIX, RSDEPS_HELPER_KEY_OUTPUT, RSDEPS_HELPER_RESTORE_KEYS_OUTPUT,
        rsdeps_output_ordinal,
    };
    let rsdeps = |name: &str| {
        rsdeps_output_ordinal(name).is_some() && !name.starts_with(RSDEPS_EXACT_OUTPUT_PREFIX)
    };
    if let Some(name) = inner.strip_prefix("steps.cache-keys.outputs.") {
        return rsdeps(name)
            || name == RSDEPS_HELPER_KEY_OUTPUT
            || name == RSDEPS_HELPER_RESTORE_KEYS_OUTPUT;
    }
    inner
        .strip_prefix("needs.plan.outputs.")
        .is_some_and(rsdeps)
}

/// Triggering-run identities the Cache seed workflow reads.
const SEED_RUN_EXPRESSIONS: [&str; 2] = [
    "github.event.workflow_run.head_sha",
    "github.event.workflow_run.id",
];

/// True for a `matrix.*` field reference (both layers allow the family).
fn is_matrix_field(inner: &str) -> bool {
    inner.strip_prefix("matrix.").is_some_and(|field| {
        !field.is_empty()
            && field
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// True for a `hashFiles('...')` call over quoted fixed paths.
///
/// Action cache keys only: the file list carries quotes, commas, dots,
/// slashes, dashes, underscores, globs, and alphanumerics, nothing else.
/// Each quoted pattern may start with one exclusion marker.
fn is_hash_files(inner: &str) -> bool {
    inner
        .strip_prefix("hashFiles(")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|files| {
            files.split(',').all(|argument| {
                argument
                    .strip_prefix('\'')
                    .and_then(|pattern| pattern.strip_suffix('\''))
                    .is_some_and(|pattern| {
                        let path = pattern.strip_prefix('!').unwrap_or(pattern);
                        !path.is_empty()
                            && path.bytes().all(|b| {
                                b.is_ascii_alphanumeric()
                                    || matches!(b, b'.' | b'/' | b'-' | b'_' | b'*')
                            })
                    })
            })
        })
}

/// Reject unlisted `${{ }}` spans in one shell-step env value.
/// # Errors
pub(crate) fn check_env_value(key: &str, value: &str) -> Result<(), RenderError> {
    check_env_value_with_scope(key, value, false)
}

/// Validate env expressions scoped to a generated composite action body.
pub(crate) fn check_composite_env_value(key: &str, value: &str) -> Result<(), RenderError> {
    check_env_value_with_scope(key, value, true)
}

fn check_env_value_with_scope(key: &str, value: &str, composite: bool) -> Result<(), RenderError> {
    let Some(spans) = expression_spans(value) else {
        return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
    };
    for inner in spans {
        let composite_input = composite
            && inner.strip_prefix("inputs.").is_some_and(|name| {
                name == crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT
                    || is_task_execution_input(name)
            });
        let workflow_input = !composite && is_workflow_input(inner);
        if !ENV_EXPRESSIONS.contains(&inner)
            && !is_matrix_field(inner)
            && !composite_input
            && !workflow_input
            && !is_github_token_secret(inner)
            && !is_seed_env(key, inner)
        {
            return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
        }
    }
    Ok(())
}

/// Cache seed env: the triggering head and the seed keys.
fn is_seed_env(key: &str, inner: &str) -> bool {
    match key {
        "VELNOR_SEED_HEAD_SHA" => inner == SEED_RUN_EXPRESSIONS[0],
        "VELNOR_SEED_KEY" | "VELNOR_SEED_HELPER_KEY" => is_rsdeps_key_reference(inner),
        _ => false,
    }
}

fn is_task_execution_input(name: &str) -> bool {
    name == "digest"
}

/// True for a validated `inputs.<name>` workflow-dispatch input reference.
fn is_workflow_input(inner: &str) -> bool {
    inner.strip_prefix("inputs.").is_some_and(|name| {
        let mut bytes = name.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        name.len() <= 64
            && (first.is_ascii_alphabetic() || first == b'_')
            && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

/// Declared provider token inputs use an uppercase Actions secret handle.
fn is_github_token_secret(inner: &str) -> bool {
    inner.strip_prefix("secrets.").is_some_and(|name| {
        name.starts_with("GH_TOKEN_")
            && name.len() <= 100
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

/// Reject bad action `with:` keys: empty, `${{`, or control characters.
/// # Errors
pub(crate) fn check_with_key(key: &str) -> Result<(), RenderError> {
    if key.is_empty()
        || key.contains("${{")
        || key.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(RenderError::BadActionRef(format!("bad_with_key:{key}")));
    }
    Ok(())
}

/// Reject control characters plus unlisted `${{ }}` in `with:` values.
///
/// Newlines stay legal: `actions/cache` takes newline-separated path
/// lists (the YAML emitter double-quotes them safely). Nul and CR are
/// never legitimate input content.
/// # Errors
pub(crate) fn check_with_value(key: &str, value: &str) -> Result<(), RenderError> {
    if value.chars().any(|ch| ch == '\0' || ch == '\r') {
        return Err(RenderError::BadActionRef(format!("bad_with_value:{key}")));
    }
    let Some(spans) = expression_spans(value) else {
        return Err(RenderError::BadActionRef(format!(
            "bad_with_expression:{key}"
        )));
    };
    for inner in spans {
        if !WITH_EXPRESSIONS.contains(&inner)
            && !is_matrix_field(inner)
            && !is_hash_files(inner)
            && !is_tofu_providers_plan_key(inner)
            && !is_rsdeps_key_reference(inner)
            && !SEED_RUN_EXPRESSIONS.contains(&inner)
            && !(key == "github-token" && inner == "github.token")
        {
            return Err(RenderError::BadActionRef(format!(
                "bad_with_expression:{key}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_with_value;

    #[test]
    fn check_with_value_accepts_glob_hash_files() {
        assert!(
            check_with_value(
                "cache-key",
                "prefix-${{ hashFiles('**/Cargo.lock','**/Cargo.toml') }}"
            )
            .is_ok()
        );
    }

    #[test]
    fn check_with_value_accepts_quoted_hash_file_exclusions() {
        assert!(
            check_with_value(
                "cache-key",
                "prefix-${{ hashFiles('**/Cargo.toml','!target/**') }}"
            )
            .is_ok()
        );
    }

    #[test]
    fn check_with_value_rejects_nonliteral_hash_file_arguments() {
        for arguments in [
            "",
            "''",
            "'!'",
            "'!!target/**'",
            "!'Cargo.toml'",
            "'Cargo.toml',",
            "'Cargo.toml'||'other'",
            "github.token",
        ] {
            let value = format!("prefix-${{{{ hashFiles({arguments}) }}}}");
            assert!(check_with_value("cache-key", &value).is_err(), "{value}");
        }
    }

    #[test]
    fn check_with_value_rejects_job_identity() {
        for value in [
            "prefix-${{ github.job }}",
            "prefix-${{ runner.environment }}",
        ] {
            assert!(check_with_value("key", value).is_err(), "{value}");
        }
    }
}
