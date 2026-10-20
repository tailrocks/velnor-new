//! Private-source credential binding gate (HD-7).
//!
//! The App installation token and the npm registry read token may be bound
//! only in the Velnor-owned fetch and revoke steps; the only other place a
//! secret may be named is the mint step's `private-key` input. Everything
//! else fails closed as `credential_binding_outside_private_fetch`.
//!
//! Declared from `support_tokens.rs` (`#[path]`, no `lib.rs` edit).

use std::collections::BTreeMap;

use velnor_actions_contract::{
    PRIVATE_FETCH_TOKEN_ENV, PRIVATE_NPM_READ_EXPR, PRIVATE_NPM_READ_TOKEN_ENV,
    PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepKind, StepRole,
};

use crate::{RenderError, toolchain_env::STEP_PRIVATE_FETCH_ALLOWLIST};

/// Mint inputs the generator may emit; anything else is a shape error.
const MINT_INPUTS: [&str; 6] = [
    "client-id",
    "private-key",
    "owner",
    "repositories",
    "permission-contents",
    "skip-token-revoke",
];

/// True for the roles allowed to bind a private-source credential.
pub(super) fn is_private_binding_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(StepRole::PrivateFetch | StepRole::PrivateRevoke | StepRole::BunRegistryInstall)
    )
}

fn step_env(step: &Step) -> &BTreeMap<String, String> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::TaskExecution { env, .. }
        | StepKind::Internal { env, .. } => env,
    }
}

/// Every text a step could carry a token expression in.
fn step_texts(step: &Step) -> Vec<&str> {
    let mut texts: Vec<&str> = step_env(step).values().map(String::as_str).collect();
    match &step.kind {
        StepKind::Action { with, .. } => texts.extend(with.values().map(String::as_str)),
        StepKind::Shell { run, .. } => texts.extend(run.iter().map(String::as_str)),
        StepKind::TaskExecution { argv, .. } => texts.extend(argv.iter().map(String::as_str)),
        StepKind::Internal { .. } => {}
    }
    texts
}

/// Reject a private-source binding outside its two steps, and require the
/// exact binding inside them.
pub(super) fn check_private_binding(id: &str, step: &Step) -> Result<(), RenderError> {
    let outside = |name: &str| {
        RenderError::InvalidWorkflow(format!(
            "credential_binding_outside_private_fetch:{id}:{name}"
        ))
    };
    if step.role == Some(StepRole::PrivateTokenMint) {
        return check_mint(id, step);
    }
    let env = step_env(step);
    if !is_private_binding_role(step.role) {
        let keyed = STEP_PRIVATE_FETCH_ALLOWLIST
            .iter()
            .any(|key| env.contains_key(*key));
        let expression = step_texts(step)
            .iter()
            .any(|text| text.contains("steps.deps-token.outputs"));
        if keyed || expression {
            return Err(outside(&step.name));
        }
        return Ok(());
    }
    for (key, value) in env {
        let registry = step.role == Some(StepRole::BunRegistryInstall);
        let expected = match key.as_str() {
            PRIVATE_FETCH_TOKEN_ENV if !registry => PRIVATE_TOKEN_OUTPUT_EXPR,
            PRIVATE_NPM_READ_TOKEN_ENV if registry => PRIVATE_NPM_READ_EXPR,
            PRIVATE_FETCH_TOKEN_ENV | PRIVATE_NPM_READ_TOKEN_ENV => {
                return Err(outside(&step.name));
            }
            _ => continue,
        };
        if value != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "private_binding_mismatch:{id}:{}",
                step.name
            )));
        }
    }
    Ok(())
}

/// Validate the mint step's exact input shape.
fn check_mint(id: &str, step: &Step) -> Result<(), RenderError> {
    let shape =
        |detail: &str| RenderError::InvalidWorkflow(format!("private_mint_shape:{id}:{detail}"));
    let StepKind::Action { with, env, .. } = &step.kind else {
        return Err(shape("not_an_action"));
    };
    if !env.is_empty() {
        return Err(shape("env"));
    }
    if let Some(key) = with.keys().find(|key| !MINT_INPUTS.contains(&key.as_str())) {
        return Err(shape(&format!("input_{key}")));
    }
    for (key, value) in [
        ("permission-contents", "read"),
        ("skip-token-revoke", "false"),
    ] {
        if with.get(key).map(String::as_str) != Some(value) {
            return Err(shape(key));
        }
    }
    if with.get("repositories").is_none_or(String::is_empty) {
        return Err(shape("repositories"));
    }
    if with.get("owner").is_none_or(String::is_empty) {
        return Err(shape("owner"));
    }
    if !with
        .get("client-id")
        .is_some_and(|v| is_expression(v, "vars"))
    {
        return Err(shape("client-id"));
    }
    if !with
        .get("private-key")
        .is_some_and(|v| is_expression(v, "secrets"))
    {
        return Err(shape("private-key"));
    }
    Ok(())
}

/// True for exactly `${{ <context>.NAME }}` with an upper-case identifier.
fn is_expression(value: &str, context: &str) -> bool {
    value
        .strip_prefix("${{ ")
        .and_then(|rest| rest.strip_suffix(" }}"))
        .and_then(|rest| rest.strip_prefix(context))
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|name| {
            !name.is_empty()
                && name.as_bytes()[0].is_ascii_uppercase()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
}
