// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    BUN_ASSETS_REPOSITORY_CHECK_OPERATION, PRIVATE_FETCH_TOKEN_ENV, PRIVATE_NPM_READ_EXPR,
    PRIVATE_NPM_READ_TOKEN_ENV, PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepKind, StepRole,
};

use crate::{RenderError, toolchain_env::STEP_PRIVATE_FETCH_ALLOWLIST};

const MINT_INPUTS: [&str; 6] = [
    "client-id",
    "private-key",
    "owner",
    "repositories",
    "permission-contents",
    "skip-token-revoke",
];

static EMPTY_ENV: std::sync::LazyLock<BTreeMap<String, String>> =
    std::sync::LazyLock::new(BTreeMap::new);

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
        StepKind::NativeCheckExecution { .. } => &EMPTY_ENV,
    }
}

fn step_texts(step: &Step) -> Vec<&str> {
    let mut texts: Vec<&str> = step_env(step).values().map(String::as_str).collect();
    match &step.kind {
        StepKind::Action { with, .. } => texts.extend(with.values().map(String::as_str)),
        StepKind::Shell { run, .. } => texts.extend(run.iter().map(String::as_str)),
        StepKind::TaskExecution { argv, .. } => texts.extend(argv.iter().map(String::as_str)),
        StepKind::Internal { .. } | StepKind::NativeCheckExecution { .. } => {}
    }
    texts
}

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
    if let StepKind::Internal { operation, env } = &step.kind {
        let repository_check = operation == BUN_ASSETS_REPOSITORY_CHECK_OPERATION
            && step.role == Some(StepRole::BunAssetsHandoff)
            && is_bun_assets_job_id(id)
            && env.len() == 2
            && env.get("GH_TOKEN").map(String::as_str) == Some("${{ github.token }}")
            && env.get("VELNOR_BUN_ASSET_REPOSITORY").map(String::as_str)
                == Some("${{ github.repository }}");
        if repository_check {
            return Ok(());
        }
        if (step.role == Some(StepRole::BunAssetsHandoff) || operation.starts_with("bun-assets-"))
            && env.contains_key("GH_TOKEN")
        {
            return Err(outside(&step.name));
        }
    }
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

fn is_bun_assets_job_id(id: &str) -> bool {
    id.strip_prefix("bun-")
        .and_then(|id| {
            id.strip_suffix(crate::verification_jobs::bun_assets_jobs::BUN_ASSETS_SUFFIX)
        })
        .is_some_and(|id| !id.is_empty())
}

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
