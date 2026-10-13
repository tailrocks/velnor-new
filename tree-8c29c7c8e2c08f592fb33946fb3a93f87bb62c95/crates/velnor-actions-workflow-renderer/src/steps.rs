//! Fixed step templates over validated command strings.
//!
//! Templates fix names, env keys, and payload shapes. Action refs and command
//! argv arrive as validated strings; nothing here builds shell or stack syntax.
//! Internal plan/merge steps travel only via env plus a request file: no
//! private subcommand string ever reaches rendered YAML.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands};

/// Env key selecting the staged-binary internal operation.
pub const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Env key carrying the JSON request-file path.
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
/// Planner operation name.
pub const PLAN_OPERATION: &str = "plan-v1";
/// Report-merge operation name.
pub const MERGE_OPERATION: &str = "merge-v1";
/// Required prefix of the digest-verified staged binary path.
pub const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
/// Required prefix of internal request directories.
pub const REQUEST_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/";
/// Env key carrying the downloaded asset SHA-256.
pub const ASSET_SHA_ENV: &str = "VELNOR_ASSET_SHA256";
/// Env key carrying the downloaded asset URL.
pub const ASSET_URL_ENV: &str = "VELNOR_ASSET_URL";
/// Substrings that must never appear in rendered YAML.
pub const FORBIDDEN_TOKENS: &[&str] = &["__internal", "velnor-actions __", "velnor-actions run"];

/// Reject text containing a private-subcommand token.
///
/// # Errors
///
/// Returns [`RenderError::PrivateSubcommand`] naming the token found.
pub fn scan_for_private_subcommands(text: &str) -> Result<(), RenderError> {
    for token in FORBIDDEN_TOKENS {
        if text.contains(token) {
            return Err(RenderError::PrivateSubcommand((*token).to_owned()));
        }
    }
    Ok(())
}

/// Validate an `owner/repo@<40 hex>` action ref; branches are rejected.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for malformed or unpinned refs.
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-") || name == "taiki-e/install-action" {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
    }
    Ok(())
}
/// Checkout step without persisted credentials.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] unless `uses` is a pinned
/// `actions/checkout` ref.
pub fn checkout_step(uses: &str) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!("not_checkout:{uses}")));
    }
    let mut with = BTreeMap::new();
    with.insert("persist-credentials".to_owned(), "false".to_owned());
    Ok(Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated pinned-action step.
///
/// # Errors
///
/// Returns [`RenderError::BadActionRef`] for bad refs or names, or
/// [`RenderError::PrivateSubcommand`] for leaked tokens.
pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    validate_uses(uses)?;
    scan_for_private_subcommands(name)?;
    scan_for_private_subcommands(uses)?;
    for (key, value) in &with {
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
    }
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated fixed-argv shell step.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
pub fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Shell { run: argv, env },
    })
}

/// Acquire-Velnor step: digest-verified staging under runner temp.
///
/// Requires `VELNOR_ASSET_SHA256` (64 hex) and an `https` asset URL in env,
/// plus a staged-binary path under `$RUNNER_TEMP` in argv.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
pub fn acquire_velnor_step(
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    match env.get(ASSET_SHA_ENV) {
        Some(sha) if sha.len() == 64 && is_lower_hex(sha) => {}
        _ => return Err(RenderError::BadCommand("bad_asset_sha256".to_owned())),
    }
    match env.get(ASSET_URL_ENV) {
        Some(url) if url.starts_with("https://") => {}
        _ => return Err(RenderError::BadCommand("bad_asset_url".to_owned())),
    }
    if !argv.iter().any(|arg| arg.contains(STAGED_BINARY_PREFIX)) {
        return Err(RenderError::BadCommand("unstaged_binary".to_owned()));
    }
    shell_step("Acquire Velnor", argv, env)
}

/// Internal plan/merge step; operation travels via env, never argv.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for unknown operations.
pub fn internal_step(name: &str, operation: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    if operation != PLAN_OPERATION && operation != MERGE_OPERATION {
        return Err(RenderError::BadCommand(format!(
            "unknown_internal_op:{operation}"
        )));
    }
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Internal {
            operation: operation.to_owned(),
        },
    })
}

/// Fixed planner step (`plan-v1`).
#[must_use]
pub fn plan_step() -> Step {
    Step {
        name: "Plan".to_owned(),
        kind: StepKind::Internal {
            operation: PLAN_OPERATION.to_owned(),
        },
    }
}

/// Fixed report-merge step (`merge-v1`).
#[must_use]
pub fn merge_step() -> Step {
    Step {
        name: "Merge reports".to_owned(),
        kind: StepKind::Internal {
            operation: MERGE_OPERATION.to_owned(),
        },
    }
}
/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

/// True for 64-char lowercase hex.
fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
