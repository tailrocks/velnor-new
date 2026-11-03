// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_family::RootSlug;
use super::producer::DEPENDENCY_SEED_JOB_CONDITION;

pub const STATE_PRODUCER_JOB_ID_PREFIX: &str = "state-producer-";

pub const STATE_SEAL_JOB_ID_PREFIX: &str = "state-seal-";

pub const STATE_PRODUCER_ARTIFACT_OUTPUT: &str = "artifact_id";

pub const STATE_PRODUCER_PRODUCED_OUTPUT: &str = "produced";

pub const STATE_PRODUCER_LANE_CONDITION: &str =
    "steps.mbx-bundle-import.outputs.producer_skip != 'true'";

pub const STATE_UPLOAD_CONDITION: &str = "steps.mbx-bundle-export.outputs.exported == 'true'";

pub const STATE_PRODUCED_CONDITION: &str = "steps.mbx-bundle-export.outputs.exported == 'true' && steps.state-seal-upload.outcome == 'success'";

pub const STATE_SEAL_ADMITTED_OUTPUT: &str = "admitted";

pub const STATE_SEAL_BUDGET_BYTES: u64 = 8_000_000_000;

pub const STATE_SEAL_SCOPE_ENV: &str = "VELNOR_STATE_SEAL_SCOPE";

pub const STATE_SEAL_PENDING_ENV: &str = "VELNOR_STATE_PENDING";

pub const STATE_SEAL_PENDING_BYTES_ENV: &str = "VELNOR_STATE_PENDING_BYTES";

pub const STATE_SEAL_ARTIFACT_ENV: &str = "VELNOR_STATE_ARTIFACT_ID";

pub const STATE_SEAL_PRODUCE_SUFFIX: &str = "_produce";

pub const STATE_SEAL_COMPAT_SUFFIX: &str = "_compat";

pub const STATE_SEAL_PREDICATE_TYPE: &str = "https://velnor.dev/attestation/state-seal/v1";

pub const STATE_SEAL_PREDICATE: &str = "{\"schema\":1}";

pub const STATE_SEAL_ARTIFACT_RETENTION_DAYS: u32 = 1;

pub const STATE_SEAL_SUBJECT_REL: &str = "velnor/state-seal/subject";

pub const STATE_SEAL_SUBJECT_EXPR: &str = "${{ runner.temp }}/velnor/state-seal/subject";

pub const STATE_SEAL_KEY_EXPR: &str = "${{steps.state-seal-subject.outputs.key}}";

pub const STATE_SEAL_SUBJECT_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true'";

pub const STATE_SEAL_KEY_ENV: &str = "VELNOR_STATE_KEY";

pub const STATE_SEAL_SAVE_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true' && steps.state-seal-admit.outputs.admitted == 'true'";

pub const STATE_SEAL_RETIRE_CONDITION: &str = "steps.state-seal-save.outcome == 'success'";

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
pub fn state_producer_job_id(root: &RootSlug) -> String {
    format!("{STATE_PRODUCER_JOB_ID_PREFIX}{root}")
}

#[must_use]
pub fn state_seal_job_id(root: &RootSlug) -> String {
    format!("{STATE_SEAL_JOB_ID_PREFIX}{root}")
}

#[must_use]
pub fn state_producer_condition(root: &str) -> String {
    format!(
        "{DEPENDENCY_SEED_JOB_CONDITION} && {}",
        state_produce_plan_test("union", root)
    )
}

#[must_use]
pub fn state_seal_condition(producer_job_id: &str) -> String {
    format!(
        "!cancelled() && {DEPENDENCY_SEED_JOB_CONDITION} && needs.{producer_job_id}.outputs.{STATE_PRODUCER_PRODUCED_OUTPUT} == 'true' && needs.{producer_job_id}.outputs.{STATE_PRODUCER_ARTIFACT_OUTPUT} != ''"
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

fn state_name_valid(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[must_use]
pub fn state_produce_output(unit: &str, slug: &str) -> String {
    format!(
        "{STATE_SEAL_ATTESTED_PREFIX}{unit}_{}{STATE_SEAL_PRODUCE_SUFFIX}",
        slug.replace('-', "_")
    )
}

#[must_use]
pub fn is_state_produce_output(name: &str) -> bool {
    name.strip_prefix(STATE_SEAL_ATTESTED_PREFIX)
        .and_then(|rest| rest.strip_suffix(STATE_SEAL_PRODUCE_SUFFIX))
        .is_some_and(|middle| {
            ["union_", "suite_"]
                .iter()
                .any(|unit| middle.strip_prefix(unit).is_some_and(state_name_valid))
        })
}

#[must_use]
pub fn state_produce_plan_expr(unit: &str, slug: &str) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        state_produce_output(unit, slug)
    )
}

#[must_use]
pub fn state_produce_plan_test(unit: &str, slug: &str) -> String {
    format!(
        "needs.plan.outputs.{} == 'true'",
        state_produce_output(unit, slug)
    )
}

#[must_use]
pub fn state_compat_output(slug: &str) -> String {
    format!(
        "{STATE_SEAL_ATTESTED_PREFIX}{}{STATE_SEAL_COMPAT_SUFFIX}",
        slug.replace('-', "_")
    )
}

#[must_use]
pub fn is_state_compat_output(name: &str) -> bool {
    name.strip_prefix(STATE_SEAL_ATTESTED_PREFIX)
        .and_then(|rest| rest.strip_suffix(STATE_SEAL_COMPAT_SUFFIX))
        .is_some_and(state_name_valid)
}

#[must_use]
pub fn state_compat_plan_expr(slug: &str) -> String {
    format!("${{{{needs.plan.outputs.{}}}}}", state_compat_output(slug))
}
