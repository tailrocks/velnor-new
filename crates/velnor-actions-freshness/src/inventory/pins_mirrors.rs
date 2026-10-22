//! Cross-file pin mirrors that keep runtime and rendered boundaries aligned.

use serde_json::Value;

use crate::context::FreshnessContext;

use super::super::{CAPABILITIES, RENDERER, RENDERER_STEPS, TOOLS};
use super::compare_pin;

pub(super) fn check_local_mirrors(ctx: &mut FreshnessContext) {
    ctx.tool_pinned = ctx
        .tools
        .iter()
        .filter_map(|tool| {
            Some((
                tool.get("name")?.as_str()?.to_owned(),
                tool.get("pinned")?.clone(),
            ))
        })
        .collect();
    let value = ctx.rust_const(CAPABILITIES, "JACTIONLINT_VERSION");
    let expected = ctx
        .tool_pinned
        .get("jactionlint")
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "tool jactionlint mirror (capabilities.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
    let value = ctx.rust_const(TOOLS, "SHELLCHECK_VERSION");
    let expected = ctx
        .tool_pinned
        .get("shellcheck")
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "tool shellcheck mirror (tools.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
    let value = ctx.rust_const(RENDERER, "ALINT_BINARY_VERSION");
    let expected = ctx
        .action_pinned
        .get("asamarts/alint")
        .and_then(|action| action.get("pinned_version"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    compare_pin(
        ctx,
        "action asamarts/alint binary mirror (render.rs)",
        value.as_deref(),
        expected.as_deref(),
    );
    check_renderer_action_ref(
        ctx,
        "actions/attest-build-provenance",
        "ATTEST_BUILD_PROVENANCE_USES",
    );
    check_renderer_action_ref(
        ctx,
        "actions/create-github-app-token",
        "CREATE_GITHUB_APP_TOKEN_USES",
    );
}

fn check_renderer_action_ref(ctx: &mut FreshnessContext, key: &str, symbol: &str) {
    // The action-pin adapter owns the reviewed pin; the renderer carries a
    // literal immutable ref that this check independently matches.
    let value = ctx.rust_const(RENDERER_STEPS, symbol);
    let sha = ctx
        .action_pinned
        .get(key)
        .and_then(|action| action.get("pinned_sha"))
        .and_then(Value::as_str);
    let expected = sha.map(|sha| format!("{key}@{sha}"));
    compare_pin(
        ctx,
        &format!("action {key} renderer source ref ({RENDERER_STEPS}::{symbol})"),
        value.as_deref(),
        expected.as_deref(),
    );
}
