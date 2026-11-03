// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checkout_history::CheckoutHistory;
use super::step::StepKind;
use super::step_identity::{
    TOOL_SEED_USES, TOOLS_CACHE_IDENTITY_DIGEST_INPUT, TOOLS_CACHE_PATHS, TOOLS_CACHE_PRELUDE_USES,
    TOOLS_CACHE_RESTORE_ACTION_USES, TOOLS_CACHE_RESTORE_USES, TOOLS_CACHE_SCRUB_USES,
    TOOLS_CACHE_V2_KEY_EXPRESSION,
};

pub const TOOLS_RESTORE_LOOKUP_ONLY_INPUT: &str = "lookup-only";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolsRestoreForm {
    Composite,
    LookupOnly,
}

impl ToolsRestoreForm {
    #[must_use]
    pub fn of(kind: &StepKind) -> Option<Self> {
        let StepKind::Action { uses, with, env } = kind else {
            return None;
        };
        if uses == TOOLS_CACHE_RESTORE_USES {
            Some(Self::Composite)
        } else if uses == TOOLS_CACHE_RESTORE_ACTION_USES
            && env.is_empty()
            && with.len() == 3
            && with.get("key").map(String::as_str) == Some(TOOLS_CACHE_V2_KEY_EXPRESSION)
            && with.get("path") == Some(&TOOLS_CACHE_PATHS.join("\n"))
            && with
                .get(TOOLS_RESTORE_LOOKUP_ONLY_INPUT)
                .is_some_and(|value| value == "true")
        {
            Some(Self::LookupOnly)
        } else {
            None
        }
    }
}

pub(super) fn valid_tools_cache_scrub(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if uses == TOOLS_CACHE_SCRUB_USES
            && with.len() == 1
            && with
                .get("mise-sha256")
                .is_some_and(|sha| crate::ids::is_lower_hex_len(sha, 64))
            && env.is_empty())
}

pub(super) fn valid_mise_setup(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if uses.starts_with("jdx/mise-action@")
            && with.get("version").is_some_and(|value| !value.is_empty())
            && with.get("sha256").is_some_and(|value| !value.is_empty())
            && with.get("install").is_some_and(|value| value == "false")
            && with.get("env").is_some_and(|value| value == "false")
            && with.get("cache").is_some_and(|value| value == "false" || value == "true")
            && with.get("cache_save").is_some_and(|value| value == "false" || value == "true"))
}

pub(super) fn valid_tool_seed_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if uses == TOOL_SEED_USES
            && with.len() == 1
            && with.get("cache_key").is_some_and(|key| !key.is_empty())
            && env.is_empty())
}

pub(super) fn valid_tools_cache_identity(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
        if TOOLS_CACHE_PRELUDE_USES.contains(&uses.as_str())
            && with.len() == 1
            && with
                .get(TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
                .is_some_and(|digest| crate::config::project_tool_refs::is_tools_identity_input(digest))
            && env.is_empty())
}

pub(super) fn valid_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if uses.starts_with("actions/checkout@")
            && with.get("persist-credentials").is_some_and(|value| value == "false"))
}

pub(super) fn valid_configured_checkout_payload(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { with, env, .. }
        if valid_checkout_payload(kind)
            && with.iter().all(|(key, value)| match key.as_str() {
                "persist-credentials" | "sparse-checkout-cone-mode" => value == "false",
                "fetch-depth" => CheckoutHistory::parse_fetch_depth(value).is_some(),
                "lfs" => value == "true",
                "sparse-checkout" => crate::config::is_sparse_checkout_exclusion(value),
                _ => false,
            })
            && env.is_empty())
}
