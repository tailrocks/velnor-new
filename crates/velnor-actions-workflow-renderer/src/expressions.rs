// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

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

const ENV_EXPRESSIONS: [&str; 15] = [
    "job.status",
    "runner.temp",
    "format('refs/heads/{0}', github.event.repository.default_branch)",
    "github.event.pull_request.head.repo.fork",
    "github.ref_name",
    "github.event_name",
    "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read'",
    "((github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true) || (github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork)) && 'write' || 'read'",
    "secrets.CARGO_REGISTRY_TOKEN",
    "secrets.GITHUB_TOKEN",
    "github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.sha",
    "inputs.cache_key",
    "steps.aws-credentials.outputs.aws-access-key-id",
    "steps.aws-credentials.outputs.aws-secret-access-key",
    "steps.aws-credentials.outputs.aws-session-token",
];

const WITH_EXPRESSIONS: [&str; 18] = [
    "runner.temp",
    "github.run_id",
    "github.run_attempt",
    "env.VELNOR_RUN_KEY_ATTEMPT",
    "github.event_name == 'push'",
    "github.event_name == 'pull_request' && !github.event.pull_request.head.repo.fork",
    "steps.v2.outputs.identity",
    "steps.v2.outputs.seed_admitted",
    "runner.environment",
    "github.job",
    "steps.tofu-providers.outputs.cache-key",
    "steps.tofu-providers.outputs.cache-path",
    "steps.cache-keys.outputs.sources_key",
    "steps.cache-keys.outputs.sources_restore_keys",
    "needs.plan.outputs.sources_key",
    "needs.plan.outputs.sources_restore_keys",
    "steps.mbx-bundle-key.outputs.key",
    "steps.mbx-bundle-key.outputs.restore_keys",
];

fn is_image_publish_output(inner: &str) -> bool {
    inner
        .strip_prefix("steps.image.outputs.")
        .is_some_and(|name| matches!(name, "digest" | "digest_amd64" | "digest_arm64"))
}

fn is_family_key_output(inner: &str) -> bool {
    use velnor_actions_contract::workflow::cache_family_steps::{FAMILY_KEYS_ID, family_for_stem};
    inner
        .strip_prefix(&format!("steps.{FAMILY_KEYS_ID}.outputs."))
        .and_then(|name| {
            name.strip_suffix("_restore_keys")
                .or_else(|| name.strip_suffix("_key"))
        })
        .is_some_and(|stem| family_for_stem(stem).is_some())
}

fn is_mbx_bundle_reader_key(inner: &str) -> bool {
    use velnor_actions_contract::workflow::cache_family_mbx::{
        is_mbx_bundle_key_needs_inner, is_mbx_bundle_restore_keys_needs_inner,
    };
    is_mbx_bundle_key_needs_inner(inner) || is_mbx_bundle_restore_keys_needs_inner(inner)
}

fn is_tofu_providers_plan_key(inner: &str) -> bool {
    inner
        .strip_prefix("needs.plan.outputs.")
        .is_some_and(|name| {
            name.starts_with(
                velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_KEY_OUTPUT_PREFIX,
            ) && velnor_actions_contract::workflow::cache_family::is_plan_cache_output(name)
        })
}

fn is_matrix_field(inner: &str) -> bool {
    inner.strip_prefix("matrix.").is_some_and(|field| {
        !field.is_empty()
            && field
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

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

pub(crate) fn check_env_value(key: &str, value: &str) -> Result<(), RenderError> {
    check_env_value_with_scope(key, value, false)
}

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
            && !is_private_env_binding(key, inner)
        {
            return Err(RenderError::BadCommand(format!("bad_env_expression:{key}")));
        }
    }
    Ok(())
}

fn is_private_env_binding(key: &str, inner: &str) -> bool {
    (key == velnor_actions_contract::PRIVATE_FETCH_TOKEN_ENV
        && inner == "steps.deps-token.outputs.token")
        || (key == velnor_actions_contract::PRIVATE_NPM_READ_TOKEN_ENV && inner == "github.token")
}

fn is_private_mint_input(key: &str, inner: &str) -> bool {
    let handle = |context: &str| {
        inner
            .strip_prefix(context)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|name| {
                name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                    && name.len() <= 100
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            })
    };
    (key == "client-id" && handle("vars")) || (key == "private-key" && handle("secrets"))
}

fn is_task_execution_input(name: &str) -> bool {
    name == "digest"
}

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

fn is_github_token_secret(inner: &str) -> bool {
    inner.strip_prefix("secrets.").is_some_and(|name| {
        name.starts_with("GH_TOKEN_")
            && name.len() <= 100
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

pub(crate) fn check_with_key(key: &str) -> Result<(), RenderError> {
    if key.is_empty()
        || key.contains("${{")
        || key.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(RenderError::BadActionRef(format!("bad_with_key:{key}")));
    }
    Ok(())
}

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
            && !is_mbx_bundle_reader_key(inner)
            && !is_private_mint_input(key, inner)
            && !is_image_publish_output(inner)
            && !is_family_key_output(inner)
        {
            return Err(RenderError::BadActionRef(format!(
                "bad_with_expression:{key}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn check_with_action_value(
    uses: &str,
    key: &str,
    value: &str,
) -> Result<(), RenderError> {
    if uses.starts_with("actions/checkout@") && key == "ref" && value == "${{ github.sha }}" {
        return Ok(());
    }
    check_with_value(key, value)
}
