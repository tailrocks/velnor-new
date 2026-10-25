use super::{absolute_env, static_tap_policy};
use crate::{
    OrchestratorError, internal::internal, retrieve_reports::staged_reads::read_staged_bytes_until,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    path::Path,
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, canonical_json_bytes, config::HomebrewTapConfig,
    package_release::PackageReleaseReceipt,
};
use velnor_actions_mise::CheckDeadline;
pub const ADMIT_HOMEBREW_TAP_OP: &str = "admit-homebrew-tap-v1";
const RECORD: &str = "tap-admission.json";
const SCHEMA: &str = "velnor.tap-admission.v1";
const LIMIT: u64 = 4 * 1024 * 1024;
const CONTEXT: &[&str] = &[
    "GITHUB_REPOSITORY",
    "GITHUB_REPOSITORY_ID",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "GITHUB_SHA",
    "GITHUB_REF",
    "GITHUB_WORKFLOW_REF",
    "GITHUB_WORKFLOW_SHA",
    "GITHUB_JOB",
    "GITHUB_EVENT_NAME",
    "VELNOR_RELEASE_COMMIT_COUNT",
    "VELNOR_RELEASE_CI_RUN_ID",
    "VELNOR_RELEASE_CI_RUN_ATTEMPT",
    "VELNOR_RELEASE_ORIGINAL_RUN_ID",
    "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    schema: String,
    pub(super) plan: PackageReleasePlan,
    release_policy: PackageReleasePolicy,
    pub(super) receipt: PackageReleaseReceipt,
    policy: HomebrewTapConfig,
    context: BTreeMap<String, Option<String>>,
}
pub fn admit_homebrew_tap_from_env() -> Result<String, OrchestratorError> {
    let deadline = deadline()?;
    deadline
        .scope(|| {
            if !env::var("GH_TOKEN").is_ok_and(|token| !token.is_empty()) {
                return Err(internal("tap_admission_read_token_missing"));
            }
            let root = absolute_env("RUNNER_TEMP", "tap_runner_temp")?.join("velnor");
            let signed = crate::package_release_signer::verify_attested_signed_package_from_env()?;
            crate::package_release_publish::verify_published_release(&signed)?;
            let release_policy = release_policy_request()?;
            release_policy
                .validate_plan(signed.plan())
                .map_err(|_| internal("tap_admission_release_policy_plan"))?;
            let record = Admission {
                schema: SCHEMA.to_owned(),
                plan: signed.plan().clone(),
                release_policy,
                receipt: signed.receipt().clone(),
                policy: static_tap_policy()?,
                context: context()?,
            };
            validate(&record, &root, deadline)?;
            let bytes =
                canonical_json_bytes(&record).map_err(|_| internal("tap_admission_encode"))?;
            if bytes.len() as u64 > LIMIT {
                return Err(internal("tap_admission_oversize"));
            }
            crate::exclusive_write::write_exclusive_until(
                &root.join(RECORD),
                &bytes,
                "tap_admission",
                || checkpoint(deadline),
            )?;
            checkpoint(deadline)?;
            Ok(super::super::sha256(&bytes))
        })
        .map_err(|e| internal(&e.to_string()))?
}

pub(super) fn load(root: &Path, deadline: CheckDeadline) -> Result<Admission, OrchestratorError> {
    let expected = env::var("VELNOR_TAP_ADMISSION_DIGEST")
        .map_err(|_| internal("tap_admission_digest_missing"))?;
    let bytes = read(root.join(RECORD).as_path(), LIMIT, deadline)?;
    let request = absolute_env("VELNOR_REQUEST_FILE", "tap_request_missing")?;
    if request != root.join("package-release-policy.json") {
        return Err(internal("tap_admission_request_path"));
    }
    let release_policy = velnor_actions_contract::parse_package_release_policy(&read(
        &request,
        1024 * 1024,
        deadline,
    )?)
    .map_err(|_| internal("tap_admission_release_policy"))?;
    parse_record(
        &bytes,
        &expected,
        &release_policy,
        &static_tap_policy()?,
        &context()?,
    )
}
fn parse_record(
    bytes: &[u8],
    digest: &str,
    release_policy: &PackageReleasePolicy,
    policy: &HomebrewTapConfig,
    context: &BTreeMap<String, Option<String>>,
) -> Result<Admission, OrchestratorError> {
    check_digest(bytes, digest)?;
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        bytes,
        usize::try_from(LIMIT).map_err(|_| internal("tap_admission_limit"))?,
    )
    .map_err(|_| internal("tap_admission_json"))?;
    let record: Admission =
        serde_json::from_value(value).map_err(|_| internal("tap_admission_shape"))?;
    if canonical_json_bytes(&record).map_err(|_| internal("tap_admission_encode"))? != bytes {
        return Err(internal("tap_admission_noncanonical"));
    }
    validate_binding(&record, release_policy, policy, context)?;
    Ok(record)
}
fn validate_binding(
    record: &Admission,
    release_policy: &PackageReleasePolicy,
    policy: &HomebrewTapConfig,
    context: &BTreeMap<String, Option<String>>,
) -> Result<(), OrchestratorError> {
    if record.schema != SCHEMA
        || record.context != *context
        || record.policy != *policy
        || record.release_policy != *release_policy
    {
        return Err(internal("tap_admission_context_changed"));
    }
    record
        .plan
        .validate()
        .map_err(|_| internal("tap_admission_plan"))?;
    record
        .release_policy
        .validate_plan(&record.plan)
        .map_err(|_| internal("tap_admission_release_policy_plan"))?;
    record
        .receipt
        .validate_against_plan(&record.plan)
        .map_err(|_| internal("tap_admission_receipt"))
}

fn validate(
    record: &Admission,
    root: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let request = absolute_env("VELNOR_REQUEST_FILE", "tap_request_missing")?;
    if request != root.join("package-release-policy.json") {
        return Err(internal("tap_admission_request_path"));
    }
    let release_policy = velnor_actions_contract::parse_package_release_policy(&read(
        &request,
        1024 * 1024,
        deadline,
    )?)
    .map_err(|_| internal("tap_admission_release_policy"))?;
    validate_binding(record, &release_policy, &static_tap_policy()?, &context()?)?;
    checkpoint(deadline)
}
fn context() -> Result<BTreeMap<String, Option<String>>, OrchestratorError> {
    CONTEXT
        .iter()
        .map(|key| match env::var(key) {
            Ok(value) => Ok(((*key).to_owned(), Some(value))),
            Err(env::VarError::NotPresent) => Ok(((*key).to_owned(), None)),
            Err(_) => Err(internal("tap_admission_context_encoding")),
        })
        .collect()
}

fn release_policy_request() -> Result<PackageReleasePolicy, OrchestratorError> {
    let raw = env::var("VELNOR_RELEASE_POLICY")
        .map_err(|_| internal("tap_admission_release_policy_missing"))?;
    velnor_actions_contract::parse_package_release_policy(raw.as_bytes())
        .map_err(|_| internal("tap_admission_release_policy_invalid"))
}
pub(super) fn deadline() -> Result<CheckDeadline, OrchestratorError> {
    CheckDeadline::from_start_with_cleanup_reserve(Instant::now(), Duration::from_secs(300))
        .map_err(|e| internal(&e.to_string()))
}
fn read(path: &Path, limit: u64, deadline: CheckDeadline) -> Result<Vec<u8>, OrchestratorError> {
    read_staged_bytes_until(path, limit, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| "tap_admission_deadline")
    })
    .map_err(|_| internal("tap_admission_read"))
}
fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|e| internal(&e.to_string()))
}
fn check_digest(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if expected.len() != 64 || super::super::sha256(bytes) != expected {
        return Err(internal("tap_admission_digest"));
    }
    Ok(())
}
pub(super) fn reject_credentials() -> Result<(), OrchestratorError> {
    credentials_absent(
        ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|key| env::var_os(key).is_some()),
    )
}
fn credentials_absent(present: bool) -> Result<(), OrchestratorError> {
    if present {
        Err(internal("tap_prepare_credentials_present"))
    } else {
        Ok(())
    }
}
