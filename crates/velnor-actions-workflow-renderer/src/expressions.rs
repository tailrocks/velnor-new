//! GitHub expression allowlists for names, env, and action inputs.
//!
//! Constructors accept `${{ }}` (narrow gates diagnose misuse with
//! specific codes; there is no blanket argv ban, and the release
//! `secret_outside_bootstrap` gate keeps diagnosing placement). Step
//! display names never legitimately carry one, and env/action values
//! only ever carry fixed runner-provided expressions, so those layers
//! fail closed on anything unlisted here.

use crate::RenderError;
use velnor_actions_contract::workflow::ir::{CACHE_MODE_PUSH_WRITE_INNER, CACHE_TRUSTED_PUSH_EXPR};

#[path = "cache_receipt_expressions.rs"]
mod cache_receipt;

#[path = "source_report_expressions.rs"]
mod source_report;

#[path = "release_helper_expressions.rs"]
mod release_helper;

#[path = "tool_report_expressions.rs"]
mod tool_report;

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

/// Helper arguments may carry only the fixed runner temporary-directory binding.
pub(crate) fn check_helper_argument(value: &str) -> Result<(), RenderError> {
    if expression_spans(value)
        .is_some_and(|spans| spans.iter().all(|inner| *inner == "runner.temp"))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(
            "source_helper_argument_expression".to_owned(),
        ))
    }
}

/// Exact `${{ }}` inners permitted in shell-step env values.
///
/// Runner paths, the release tag, plan-matrix coordinates, the two
/// fixed secret bindings (bootstrap registry plus the release forge
/// token, whose placements the release gates still police separately),
/// and the default-branch cache-mode selector (a request to the action,
/// whose transport must independently enforce its writer policy).
/// CI admission additionally binds the fixed read token and typed event SHA.
/// Run IDs remain absent (never in env).
const ENV_EXPRESSIONS: [&str; 8] = [
    "runner.temp",
    "github.ref_name",
    "github.event_name",
    "github.token",
    "github.sha",
    CACHE_MODE_PUSH_WRITE_INNER,
    "secrets.CARGO_REGISTRY_TOKEN",
    "secrets.GITHUB_TOKEN",
];

/// Exact `${{ }}` inners permitted in action `with:` values.
///
/// Run-scoped names, runner paths, matrix coordinates, the
/// push-gated cache-save flag, and the publish step's derived
/// artifact name. Notably absent: every `secrets.*` handle (rejected
/// separately as `secret_in_action_input`).
const WITH_EXPRESSIONS: [&str; 12] = [
    "runner.temp",
    "github.run_id",
    "github.run_attempt",
    CACHE_TRUSTED_PUSH_EXPR,
    "env.VELNOR_CACHE_IMAGE",
    "env.VELNOR_TOOLS_SNAPSHOT_DIGEST",
    "env.VELNOR_SOURCES_SNAPSHOT_DIGEST",
    "env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_DIGEST",
    "env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_DIGEST",
    "env.VELNOR_GRADLE_DEPENDENCIES_SNAPSHOT_DIGEST",
    "steps.publish-baseline.outputs.artifact_name",
    "steps.velnor-tool-after.outputs.digest",
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
/// slashes, dashes, and alphanumerics, nothing else.
fn is_hash_files(inner: &str) -> bool {
    inner
        .strip_prefix("hashFiles(")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|files| {
            !files.is_empty()
                && files.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || matches!(b, b'\'' | b',' | b'.' | b'/' | b'-' | b'_')
                })
        })
}

/// Reject unlisted `${{ }}` spans in one shell-step env value.
/// # Errors
pub(crate) fn check_env_value(key: &str, value: &str) -> Result<(), RenderError> {
    let Some(spans) = expression_spans(value) else {
        return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
    };
    if let Some(result) = cache_receipt::check(key, value, &spans) {
        return result;
    }
    if let Some(result) = source_report::check(key, value, &spans) {
        return result;
    }
    if let Some(result) = release_helper::check(key, value, &spans) {
        return result;
    }
    if let Some(result) = tool_report::check(key, value, &spans) {
        return result;
    }
    if matches!(key, "APT_GPG_PRIVATE_KEY" | "APT_GPG_PASSPHRASE") {
        return if value == format!("${{{{ secrets.{key} }}}}")
            && spans == [format!("secrets.{key}")]
        {
            Ok(())
        } else {
            Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
        };
    }
    if matches!(
        key,
        "ACTIONS_ID_TOKEN_REQUEST_URL" | "ACTIONS_ID_TOKEN_REQUEST_TOKEN"
    ) {
        return if value == format!("${{{{ env.{key} }}}}") && spans == [format!("env.{key}")] {
            Ok(())
        } else {
            Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
        };
    }
    if key == "VELNOR_HELPER_OUTCOME" {
        return match spans.as_slice() {
            [inner] if is_helper_outcome(inner) && value == format!("${{{{ {inner} }}}}") => Ok(()),
            _ => Err(RenderError::BadCommand(format!("bad_env_expression:{key}"))),
        };
    }
    if key == "VELNOR_ACTION_OUTCOME" {
        return match spans.as_slice() {
            [inner] if is_action_outcome(key, inner) && value == format!("${{{{ {inner} }}}}") => {
                Ok(())
            }
            _ => Err(RenderError::BadCommand(format!("bad_env_expression:{key}"))),
        };
    }
    for inner in spans {
        if !ENV_EXPRESSIONS.contains(&inner)
            && !is_matrix_field(inner)
            && !is_snapshot_restore(key, inner)
            && !is_observer_binding(key, inner)
        {
            return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
        }
    }
    Ok(())
}

/// Only fixed observer metadata keys may bind repository/run/Required identity.
fn is_observer_binding(key: &str, inner: &str) -> bool {
    matches!(
        (key, inner),
        ("GITHUB_REPOSITORY", "github.repository")
            | ("REF", "github.ref")
            | ("REF_PROTECTED", "github.ref_protected")
            | ("RUN_ID", "github.run_id")
            | ("RUN_ATTEMPT", "github.run_attempt")
    ) || (key == "REQUIRED_RESULT"
        && inner == format!("needs.{}.result", crate::render::FINAL_JOB_ID))
}

fn is_snapshot_restore(key: &str, inner: &str) -> bool {
    key == "VELNOR_SNAPSHOT_RESTORED"
        && matches!(
            inner,
            "steps.velnor-tools-cache.outputs.cache-matched-key"
                | "steps.velnor-planning-tools-cache.outputs.cache-matched-key"
                | "steps.velnor-npm-bootstrap-cache.outputs.cache-matched-key"
                | "steps.velnor-bun-bootstrap-cache.outputs.cache-matched-key"
                | "steps.velnor-tofu-bootstrap-cache.outputs.cache-matched-key"
                | "steps.velnor-gradle-bootstrap-cache.outputs.cache-matched-key"
                | "steps.velnor-sources-cache.outputs.cache-matched-key"
                | "steps.velnor-bun-cache.outputs.cache-matched-key"
                | "steps.velnor-npm-cache.outputs.cache-matched-key"
                | "steps.velnor-gradle-dependencies-cache.outputs.cache-matched-key"
        )
}

/// Only the closed container adapter may consume its validation action outcome.
/// Arbitrary step expressions stay denied.
fn is_action_outcome(key: &str, inner: &str) -> bool {
    if key != "VELNOR_ACTION_OUTCOME" {
        return false;
    }
    let Some(id) = inner
        .strip_prefix("steps.")
        .and_then(|rest| rest.strip_suffix(".outcome"))
    else {
        return false;
    };
    id.strip_prefix("velnor-action-")
        .is_some_and(|matrix| velnor_actions_contract::validate_matrix_key(matrix).is_ok())
        && velnor_actions_contract::StepId::new(id).is_ok()
}

/// Helper outcomes bind only the source-bound obligation's typed matrix ID.
fn is_helper_outcome(inner: &str) -> bool {
    let Some(id) = inner
        .strip_prefix("steps.")
        .and_then(|rest| rest.strip_suffix(".outcome"))
    else {
        return false;
    };
    id.strip_prefix("velnor-helper-")
        .is_some_and(|matrix| velnor_actions_contract::validate_matrix_key(matrix).is_ok())
        && velnor_actions_contract::StepId::new(id).is_ok()
}

#[cfg(test)]
#[path = "expressions_helper_tests.rs"]
mod helper_tests;

#[cfg(test)]
#[path = "expressions_action_tests.rs"]
mod action_tests;

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
    if crate::analysis_publication::is_output_binding(key, value) {
        return Ok(());
    }
    for inner in spans {
        if !WITH_EXPRESSIONS.contains(&inner) && !is_matrix_field(inner) && !is_hash_files(inner) {
            return Err(RenderError::BadActionRef(format!(
                "bad_with_expression:{key}"
            )));
        }
    }
    Ok(())
}
