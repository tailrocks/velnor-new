// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_family::{RootSlug, SliceId, StateUnit, is_state_output_token};
use super::producer::DEPENDENCY_SEED_JOB_CONDITION;

pub const STATE_PRODUCER_JOB_ID_PREFIX: &str = "state-producer-";

pub const STATE_SEAL_JOB_ID: &str = "state-seal";

pub const STATE_PRODUCER_PRODUCED_OUTPUT: &str = "produced";

pub const STATE_PRODUCER_MAX_PARALLEL: u32 = 2;

pub const STATE_SEAL_MAX_PARALLEL: u32 = 1;

pub const STATE_SEAL_MATRIX_OUTPUT: &str = "state_matrix_seal";

const STATE_PRODUCER_MATRIX_OUTPUT_PREFIX: &str = "state_matrix_producer_";

pub const STATE_MATRIX_SLICE: &str = "slice";

pub const STATE_MATRIX_ATTESTED: &str = "attested";

pub const STATE_MATRIX_SNAPSHOT: &str = "snapshot";

pub const STATE_MATRIX_COMPAT: &str = "compat";

pub const STATE_MATRIX_PENDING: &str = "pending";

pub const STATE_PRODUCER_LANE_CONDITION: &str =
    "steps.mbx-bundle-import.outputs.producer_skip != 'true'";

pub const STATE_UPLOAD_CONDITION: &str = "steps.mbx-bundle-export.outputs.exported == 'true'";

pub const STATE_PRODUCED_CONDITION: &str = "steps.mbx-bundle-export.outputs.exported == 'true' && steps.state-seal-upload.outcome == 'success'";

pub const STATE_SEAL_ADMITTED_OUTPUT: &str = "admitted";

pub const STATE_SEAL_BUDGET_BYTES: u64 = 8_000_000_000;

pub const STATE_SEAL_SCOPE_ENV: &str = "VELNOR_STATE_SEAL_SCOPE";

pub const STATE_SEAL_PENDING_BYTES_ENV: &str = "VELNOR_STATE_PENDING_BYTES";

pub const STATE_SEAL_WRITERS_BYTES_ENV: &str = "VELNOR_STATE_WRITERS_BYTES";

pub const STATE_SEAL_ARTIFACT_ENV: &str = "VELNOR_STATE_ARTIFACT_NAME";

pub const STATE_SEAL_COMPAT_SUFFIX: &str = "_compat";

pub const STATE_SEAL_PREDICATE_TYPE: &str = "https://velnor.dev/attestation/state-seal/v1";

pub const STATE_SEAL_PREDICATE: &str = "{\"schema\":1}";

pub const STATE_SEAL_ARTIFACT_RETENTION_DAYS: u32 = 1;

pub const STATE_SEAL_SUBJECT_REL: &str = "velnor/state-seal/subject";

pub const STATE_SEAL_SUBJECT_EXPR: &str = "${{ runner.temp }}/velnor/state-seal/subject";

pub const STATE_SEAL_KEY_EXPR: &str = "${{steps.state-seal-subject.outputs.key}}";

pub const STATE_SEAL_SUBJECT_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true'";

pub const STATE_SEAL_SAVE_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true' && steps.state-seal-admit.outputs.admitted == 'true'";

pub const STATE_SEAL_BIND_CONDITION: &str = "steps.state-seal-save.outcome == 'success'";

pub const STATE_SEAL_RETIRE_CONDITION: &str = STATE_SEAL_ATTEST_CONDITION;

pub const STATE_SEAL_TIP_CONDITION: &str = STATE_SEAL_SUBJECT_CONDITION;

pub const STATE_SEAL_ADMIT_CONDITION: &str = "steps.state-seal-subject.outputs.sealed == 'true' && steps.state-seal-tip.outputs.current == 'true'";

pub const STATE_SEAL_TIP_OUTPUT: &str = "current";

pub const STATE_SEAL_ATTEST_CONDITION: &str = "steps.state-seal-bind.outputs.bound == 'true'";

pub const STATE_SEAL_VERIFY_LIMIT: usize = 3;

pub const STATE_SEAL_LIST_LIMIT: usize = 100;

pub const STATE_SEAL_SELECT_LIST_LIMIT: usize = 1000;

pub const STATE_SEAL_SELECT_PARALLELISM: usize = 16;

pub const STATE_SEAL_BIND_ATTEMPTS: usize = 5;

pub const STATE_SEAL_BIND_PAUSE_SECONDS: u64 = 3;

pub const STATE_SEAL_LIST_FIELDS: &str = "key,id,sizeInBytes";

pub const STATE_SEAL_LIST_TAB: &str = "\t";

pub const STATE_SEAL_LIST_JQ: &str =
    ".[] | [.key, (.id | tostring), (.sizeInBytes | tostring)] | @tsv";

pub const STATE_SEAL_VERIFY_TIMEOUT_SECONDS: u64 = 60;

pub const STATE_SEAL_VERIFY_TIMEOUT_ENV: &str = "VELNOR_STATE_GH_TIMEOUT";

pub const STATE_SEAL_UNIT_TIMEOUT_SECONDS: u64 =
    STATE_SEAL_VERIFY_TIMEOUT_SECONDS * (STATE_SEAL_VERIFY_LIMIT as u64 + 1);

pub const STATE_SEAL_UNIT_TIMEOUT_ENV: &str = "VELNOR_STATE_UNIT_TIMEOUT";

pub const STATE_SEAL_KEY_ENV: &str = "VELNOR_STATE_SEAL_KEY";

pub const STATE_SEAL_PREFIX_ENV: &str = "VELNOR_STATE_SEAL_PREFIX";

pub const STATE_SEAL_ATTESTED_ENV: &str = "VELNOR_STATE_ATTESTED";

pub const STATE_SEAL_WORKFLOW_PATH: &str = ".github/workflows/state-seal.yml";

pub const STATE_SEAL_WORKFLOW_NAME: &str = "State seal";

pub const STATE_SEAL_CONCURRENCY_GROUP: &str = "velnor-state-seal-${{ github.ref }}";

pub const STATE_SEAL_CONCURRENCY_CANCEL: &str = "false";

pub const STATE_SEAL_ATTESTED_SUFFIX: &str = "_attested";

pub const STATE_SEAL_ATTESTED_PREFIX: &str = "state_";

#[must_use]
pub fn state_seal_signer_workflow() -> String {
    format!("$GITHUB_REPOSITORY/{STATE_SEAL_WORKFLOW_PATH}")
}

#[must_use]
pub fn state_producer_job_id(root: &RootSlug) -> String {
    format!("{STATE_PRODUCER_JOB_ID_PREFIX}{root}")
}

#[must_use]
pub fn state_matrix_expr(key: &str) -> String {
    format!("${{{{ matrix.{key} }}}}")
}

#[must_use]
pub fn state_producer_matrix_output(root: &RootSlug) -> String {
    format!(
        "{STATE_PRODUCER_MATRIX_OUTPUT_PREFIX}{}",
        root.as_str().replace('-', "_")
    )
}

#[must_use]
pub fn is_state_matrix_output(name: &str) -> bool {
    name == STATE_SEAL_MATRIX_OUTPUT
        || name
            .strip_prefix(STATE_PRODUCER_MATRIX_OUTPUT_PREFIX)
            .is_some_and(|root| {
                !root.is_empty()
                    && root
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
}

#[must_use]
pub fn state_producer_condition(root: &RootSlug) -> String {
    format!(
        "{DEPENDENCY_SEED_JOB_CONDITION} && needs.plan.outputs.{} != ''",
        state_producer_matrix_output(root)
    )
}

#[must_use]
pub fn state_producer_lane_condition(slice: &SliceId) -> String {
    format!("matrix.{STATE_MATRIX_SLICE} == '{slice}' && {STATE_PRODUCER_LANE_CONDITION}")
}

#[must_use]
pub fn state_producer_lane_slice(condition: &str) -> Option<SliceId> {
    condition
        .strip_prefix(&format!("matrix.{STATE_MATRIX_SLICE} == '"))
        .and_then(|rest| rest.strip_suffix(&format!(" && {STATE_PRODUCER_LANE_CONDITION}")))
        .and_then(|rest| rest.strip_suffix('\''))
        .and_then(|slice| SliceId::parse(slice).ok())
}

#[must_use]
pub fn state_payload_artifact_name() -> String {
    format!(
        "velnor-state-{}-r${{{{ github.run_id }}}}-a${{{{ env.VELNOR_RUN_KEY_ATTEMPT }}}}",
        state_matrix_expr(STATE_MATRIX_SLICE)
    )
}

#[must_use]
pub fn state_seal_condition() -> String {
    format!(
        "!cancelled() && {DEPENDENCY_SEED_JOB_CONDITION} && needs.plan.outputs.{STATE_SEAL_MATRIX_OUTPUT} != ''"
    )
}

fn state_output(unit: &StateUnit, suffix: &str) -> String {
    format!(
        "{STATE_SEAL_ATTESTED_PREFIX}{}{suffix}",
        unit.output_token()
    )
}

fn is_state_output(name: &str, suffix: &str) -> bool {
    name.strip_prefix(STATE_SEAL_ATTESTED_PREFIX)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(is_state_output_token)
}

#[must_use]
pub fn state_attested_output(unit: &StateUnit) -> String {
    state_output(unit, STATE_SEAL_ATTESTED_SUFFIX)
}

#[must_use]
pub fn is_state_attested_output(name: &str) -> bool {
    is_state_output(name, STATE_SEAL_ATTESTED_SUFFIX)
}

#[must_use]
pub fn state_attested_plan_expr(unit: &StateUnit) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        state_attested_output(unit)
    )
}

#[must_use]
pub fn state_compat_output(unit: &StateUnit) -> String {
    state_output(unit, STATE_SEAL_COMPAT_SUFFIX)
}

#[must_use]
pub fn is_state_compat_output(name: &str) -> bool {
    is_state_output(name, STATE_SEAL_COMPAT_SUFFIX)
}

#[must_use]
pub fn state_compat_plan_expr(unit: &StateUnit) -> String {
    format!("${{{{needs.plan.outputs.{}}}}}", state_compat_output(unit))
}
