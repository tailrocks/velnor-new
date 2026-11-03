// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_family::{CacheFamily, S256, StateTrust};

pub const STATE_CANARY_PRODUCER_JOB_ID: &str = "state-canary-producer";

pub const STATE_CANARY_SEAL_JOB_ID: &str = "state-canary-seal";

pub const STATE_CANARY_VERIFIED_OUTPUT: &str = "state_canary_verified";

pub const STATE_CANARY_UNVERIFIED_TEST: &str = "needs.plan.outputs.state_canary_verified != 'true'";

pub const STATE_CANARY_ARTIFACT_OUTPUT: &str = "artifact_id";

pub const STATE_CANARY_UNIT: &str = "canary";

pub const STATE_CANARY_ROOT: &str = "root";

pub const STATE_SEAL_PREDICATE_TYPE: &str = "https://velnor.dev/attestation/state-seal/v1";

pub const STATE_SEAL_PREDICATE: &str = "{\"schema\":1}";

pub const STATE_SEAL_ARTIFACT_RETENTION_DAYS: u32 = 1;

pub const STATE_SEAL_SUBJECT_REL: &str = "velnor/state-seal/subject";

pub const STATE_SEAL_SUBJECT_EXPR: &str = "${{ runner.temp }}/velnor/state-seal/subject";

pub const STATE_SEAL_KEY_EXPR: &str = "${{steps.state-seal-subject.outputs.key}}";

pub const STATE_SEAL_SAVE_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true'";

pub const STATE_SEAL_VERIFY_LIMIT: usize = 3;

pub const STATE_SEAL_LIST_LIMIT: usize = 100;

pub const STATE_SEAL_VERIFY_TIMEOUT_SECONDS: u64 = 60;

pub const STATE_SEAL_VERIFY_TIMEOUT_ENV: &str = "VELNOR_STATE_GH_TIMEOUT";

pub const STATE_SEAL_PREFIX_ENV: &str = "VELNOR_STATE_SEAL_PREFIX";

pub const STATE_SEAL_ATTESTED_ENV: &str = "VELNOR_STATE_ATTESTED";

pub const STATE_SEAL_SIGNER_REF_ENV: &str = "GITHUB_WORKFLOW_REF";

pub const STATE_SEAL_ATTESTED_SUFFIX: &str = "_attested";

pub const STATE_SEAL_ATTESTED_PREFIX: &str = "state_";

#[must_use]
pub fn state_canary_prefix() -> String {
    format!(
        "{}{}-{STATE_CANARY_UNIT}-{STATE_CANARY_ROOT}-",
        CacheFamily::MbxBundle.key_prefix(),
        StateTrust::Trusted.as_str(),
    )
}

#[must_use]
pub fn state_canary_identity_prefix() -> String {
    format!(
        "{}{}-{}-",
        state_canary_prefix(),
        S256::of(b"velnor-state-seal-canary-compat"),
        S256::of(b"velnor-state-seal-canary-snapshot"),
    )
}

#[must_use]
pub fn state_attested_output(unit: &str, slug: &str) -> String {
    format!(
        "{STATE_SEAL_ATTESTED_PREFIX}{unit}_{}{STATE_SEAL_ATTESTED_SUFFIX}",
        slug.replace('-', "_")
    )
}

#[must_use]
pub fn is_state_attested_output(name: &str) -> bool {
    name.strip_prefix(STATE_SEAL_ATTESTED_PREFIX)
        .and_then(|rest| rest.strip_suffix(STATE_SEAL_ATTESTED_SUFFIX))
        .is_some_and(|middle| {
            ["union_", "suite_"].iter().any(|unit| {
                middle.strip_prefix(unit).is_some_and(|slug| {
                    !slug.is_empty()
                        && slug
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                })
            })
        })
}

#[must_use]
pub fn state_attested_plan_expr(unit: &str, slug: &str) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        state_attested_output(unit, slug)
    )
}
