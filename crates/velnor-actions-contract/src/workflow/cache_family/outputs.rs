// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CacheFamily, ROOT_SLUG_MAX_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTrust {
    Trusted,

    Pr,

    None,
}

impl CacheTrust {
    pub const ALL: [Self; 3] = [Self::Trusted, Self::Pr, Self::None];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Pr => "pr",
            Self::None => "none",
        }
    }
}

pub const S256_DIGEST_BYTES: usize = 69;

pub const CACHE_KEYS_STEP_NAME: &str = "Compute cache keys";

pub const TRUST_OUTPUT: &str = "trust";

pub const SOURCES_KEY_OUTPUT: &str = "sources_key";

pub const SOURCES_RESTORE_KEYS_OUTPUT: &str = "sources_restore_keys";

pub const SOURCES_SAVE_OUTPUT: &str = "sources_save";

pub const SOURCES_SNAPSHOT_OUTPUT: &str = "sources_snapshot";

pub const SOURCES_PRODUCE_OUTPUT: &str = "sources_produce";

pub const SOURCES_PRODUCE_PLAN_TEST: &str = "needs.plan.outputs.sources_produce == 'true'";

pub const SOURCES_PRODUCE_STEP_EXPR: &str = "${{ steps.cache-keys.outputs.sources_save == 'true' && steps.sources-restore.outputs.cache-hit != 'true' && !endsWith(steps.sources-restore.outputs.cache-matched-key, steps.cache-keys.outputs.sources_snapshot) }}";

pub const PLAN_CACHE_TRUST_OUTPUT: &str = "cache_trust";

pub const TOFU_PROVIDERS_KEY_OUTPUT_PREFIX: &str = "tofu_providers_key_";

pub const SOURCES_KEY_STEP_EXPR: &str = "${{steps.cache-keys.outputs.sources_key}}";

pub const SOURCES_RESTORE_KEYS_STEP_EXPR: &str =
    "${{steps.cache-keys.outputs.sources_restore_keys}}";

pub const SOURCES_KEY_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_key}}";

pub const SOURCES_RESTORE_KEYS_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_restore_keys}}";

pub const SOURCES_SAVE_CONDITION: &str =
    "success() && steps.sources-restore.outputs.cache-hit != 'true'";

pub const TOFU_PROVIDERS_PROBE_MISS_CONDITION: &str =
    "steps.tofu-providers.outputs.cache-hit != 'true'";

pub const TOFU_PROVIDERS_SAVE_CONDITION: &str = "success() && needs.plan.outputs.cache_trust == 'trusted' && steps.tofu-providers.outputs.cache-hit != 'true'";

pub const PLAN_CACHE_OUTPUTS: [(&str, &str); 3] = [
    (PLAN_CACHE_TRUST_OUTPUT, TRUST_OUTPUT),
    (SOURCES_KEY_OUTPUT, SOURCES_KEY_OUTPUT),
    (SOURCES_RESTORE_KEYS_OUTPUT, SOURCES_RESTORE_KEYS_OUTPUT),
];

#[must_use]
pub fn tofu_providers_key_output(ordinal: usize) -> String {
    format!("{TOFU_PROVIDERS_KEY_OUTPUT_PREFIX}{ordinal}")
}

#[must_use]
pub fn tofu_providers_key_plan_expr(ordinal: usize) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        tofu_providers_key_output(ordinal)
    )
}

const STATE_SNAPSHOT_OUTPUT_PREFIX: &str = "state_";

const STATE_SNAPSHOT_OUTPUT_SUFFIX: &str = "_snapshot";

#[must_use]
pub fn state_snapshot_output(root: &super::RootSlug) -> String {
    format!(
        "{STATE_SNAPSHOT_OUTPUT_PREFIX}{}{STATE_SNAPSHOT_OUTPUT_SUFFIX}",
        root.as_str().replace('-', "_")
    )
}

#[must_use]
pub fn state_snapshot_plan_expr(root: &super::RootSlug) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        state_snapshot_output(root)
    )
}

#[must_use]
pub fn is_state_snapshot_output(name: &str) -> bool {
    name.strip_prefix(STATE_SNAPSHOT_OUTPUT_PREFIX)
        .and_then(|rest| rest.strip_suffix(STATE_SNAPSHOT_OUTPUT_SUFFIX))
        .is_some_and(|slug| {
            !slug.is_empty()
                && slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

#[must_use]
pub fn is_plan_cache_output(name: &str) -> bool {
    PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| *output == name)
        || is_state_snapshot_output(name)
        || name == SOURCES_PRODUCE_OUTPUT
        || name
            .strip_prefix(TOFU_PROVIDERS_KEY_OUTPUT_PREFIX)
            .is_some_and(|ordinal| {
                !ordinal.is_empty() && ordinal.bytes().all(|b| b.is_ascii_digit())
            })
}

pub const PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES: usize = 64 * 1024;

#[must_use]
pub fn cache_output_reserve_utf16_bytes(tofu_roots: usize, state_roots: usize) -> usize {
    let record = |name: &str, value: usize| (name.len() + 1 + value + 1) * 2;
    let sources = CacheFamily::Sources.max_key_bytes();
    let tofu = CacheFamily::TofuProviders.max_key_bytes();
    record(PLAN_CACHE_TRUST_OUTPUT, 7)
        + record(SOURCES_KEY_OUTPUT, sources)
        + record(SOURCES_RESTORE_KEYS_OUTPUT, 2 * sources + 1)
        + tofu_roots * record(&tofu_providers_key_output(tofu_roots), tofu)
        + state_roots * record(&"s".repeat(ROOT_SLUG_MAX_BYTES + 16), S256_DIGEST_BYTES)
}
