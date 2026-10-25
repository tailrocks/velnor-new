use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    homebrew_spec::{
        HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec, parse_generated_formula,
        render_homebrew_formula, validate_homebrew_projection,
    },
    package_release::PackageReleaseChannel,
    parse_package_release_plan,
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "homebrew_qualification_budget.rs"]
mod budget;
#[path = "homebrew_qualification_prepare.rs"]
mod prepare;
use budget::absolute_deadline;
#[path = "homebrew_qualification_record.rs"]
mod record;

use record::{HistoricalRecord, admission};

const REPOSITORY: &str = "jackin-project/jackin";
const REPOSITORY_ID: u64 = 1_197_700_841;
const PLAN: &[u8] = include_bytes!("../../../.velnor/qualification/homebrew/preview-plan.json");
const FORMULA_SPEC: &[u8] =
    include_bytes!("../../../.velnor/qualification/homebrew/formula-spec.json");
const A_RECORD: &[u8] = include_bytes!("../../../.velnor/qualification/homebrew/preview-a.json");
const D_RECORD: &[u8] = include_bytes!("../../../.velnor/qualification/homebrew/preview-d.json");
const A_ADMISSION: &[u8] =
    include_bytes!("../../../.velnor/qualification/homebrew/preview-a-source-admission.json");
const PLAN_SHA256: &str = "5f4dc66b22cbce0f309ea9ab8c93709774a8890f365e0d61277fb2b0b505d9ae";

pub fn execute_homebrew_qualification_from_env() -> Result<(), OrchestratorError> {
    let deadline = absolute_deadline()?;
    let candidate = crate::homebrew::hosted_profile::verify_release_qualification_candidate()?;
    let (plan, spec, a, d) = load_catalog()?;
    let (verified_a, verified_d) = verify_historical_pair(&plan, &a, &d, deadline)?;
    let _hosted_profile_evidence =
        crate::homebrew::hosted_profile::prepare_for_release_qualification(&candidate, deadline)?;
    install_preview_transition(&spec, &plan, &verified_a, &verified_d, &a, &d, deadline)?;
    verify_stable_fixture(&spec, &plan)
}

fn load_catalog() -> Result<
    (
        velnor_actions_contract::PackageReleasePlan,
        HomebrewFormulaSpec,
        HistoricalRecord,
        HistoricalRecord,
    ),
    OrchestratorError,
> {
    let plan =
        parse_package_release_plan(PLAN).map_err(|_| internal("homebrew_qualification_plan"))?;
    if sha256(PLAN) != PLAN_SHA256 {
        return Err(internal("homebrew_qualification_plan_digest"));
    }
    let text =
        std::str::from_utf8(FORMULA_SPEC).map_err(|_| internal("homebrew_qualification_spec"))?;
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| internal("homebrew_qualification_spec"))?;
    let spec: HomebrewFormulaSpec =
        serde_json::from_value(value).map_err(|_| internal("homebrew_qualification_spec"))?;
    spec.validate()
        .map_err(|_| internal("homebrew_qualification_spec"))?;
    let a = parse_record(A_RECORD, "A", &plan)?;
    let d = parse_record(D_RECORD, "D", &plan)?;
    if a.recovery.is_none() || d.recovery.is_some() {
        return Err(internal("homebrew_qualification_recovery_shape"));
    }
    Ok((plan, spec, a, d))
}

fn verify_historical_pair(
    plan: &velnor_actions_contract::PackageReleasePlan,
    a: &HistoricalRecord,
    d: &HistoricalRecord,
    deadline: CheckDeadline,
) -> Result<
    (
        crate::package_release_public::VerifiedPublicPackageRelease,
        crate::package_release_public::VerifiedPublicPackageRelease,
    ),
    OrchestratorError,
> {
    let current_main = crate::package_release_publish::verify_public_protected_main_lineage(
        REPOSITORY,
        REPOSITORY_ID,
        "main",
        &a.release.source_sha,
        &a.producer.authority_sha,
        deadline,
    )?;
    let policy =
        crate::package_release_public::PackageReleasePublicPolicy::from_trusted_source_authority(
            plan.clone(),
            REPOSITORY_ID,
            current_main.clone(),
        )?
        .with_historical_admissions(vec![
            admission(a, plan, Some(A_ADMISSION))?,
            admission(d, plan, None)?,
        ])?;
    let verified_a = verify(&policy, a, deadline)?;
    let verified_d = verify(&policy, d, deadline)?;
    verify_custody_pair(plan, &verified_a, &verified_d, &current_main)?;
    check_receipt(&verified_a, a)?;
    check_receipt(&verified_d, d)?;
    let final_main = crate::package_release_publish::verify_public_protected_main_lineage(
        REPOSITORY,
        REPOSITORY_ID,
        "main",
        &a.release.source_sha,
        &a.producer.authority_sha,
        deadline,
    )?;
    if final_main != current_main {
        return Err(internal("homebrew_qualification_main_tip_changed"));
    }
    Ok((verified_a, verified_d))
}

fn verify_custody_pair(
    plan: &velnor_actions_contract::PackageReleasePlan,
    verified_a: &crate::package_release_public::VerifiedPublicPackageRelease,
    verified_d: &crate::package_release_public::VerifiedPublicPackageRelease,
    current_main: &str,
) -> Result<(), OrchestratorError> {
    if !verified_a.is_historical_admission()
        || !verified_d.is_historical_admission()
        || verified_a.authority_sha() != current_main
        || verified_d.authority_sha() != current_main
        || verified_a.plan() != plan
        || verified_d.plan() != plan
    {
        return Err(internal("homebrew_qualification_verified_custody_identity"));
    }
    Ok(())
}

fn install_preview_transition(
    spec: &HomebrewFormulaSpec,
    plan: &velnor_actions_contract::PackageReleasePlan,
    verified_a: &crate::package_release_public::VerifiedPublicPackageRelease,
    verified_d: &crate::package_release_public::VerifiedPublicPackageRelease,
    a: &HistoricalRecord,
    d: &HistoricalRecord,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let runner_temp = std::env::var_os("RUNNER_TEMP")
        .ok_or_else(|| internal("homebrew_qualification_runner_temp"))?;
    let base = private_tempdir_in(Path::new(&runner_temp), "velnor-homebrew-ad")?;
    let prepared_a = prepare::make_request(spec, plan, verified_a, a, &base.path().join("a"))?;
    let prepared_d = prepare::make_request(spec, plan, verified_d, d, &base.path().join("d"))?;
    let request_a = prepared_a.request(spec, deadline);
    let request_d = prepared_d.request(spec, deadline);
    let (_evidence_a, custody_a) =
        crate::homebrew::install::install_homebrew_candidate_with_custody(&request_a)?;
    crate::homebrew::install::upgrade_homebrew_candidate(&request_d, &request_a, &custody_a)?;
    Ok(())
}

fn private_tempdir_in(parent: &Path, prefix: &str) -> Result<tempfile::TempDir, OrchestratorError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir_in(parent)
        .map_err(|error| OrchestratorError::io(parent.display().to_string(), error.to_string()))
}

fn verify_stable_fixture(
    spec: &HomebrewFormulaSpec,
    plan: &velnor_actions_contract::PackageReleasePlan,
) -> Result<(), OrchestratorError> {
    let stable_channel = spec
        .channels
        .iter()
        .find(|c| c.channel == PackageReleaseChannel::Stable)
        .ok_or_else(|| internal("homebrew_qualification_stable_fixture_missing"))?;
    let mut stable_plan = plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let stable_projection = validate_homebrew_projection(spec, &stable_plan)
        .map_err(|_| internal("homebrew_qualification_stable_projection"))?;
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(spec, &stable_projection)
        .map_err(|_| internal("homebrew_qualification_stable_sentinel"))?;
    let stable_bytes = render_homebrew_formula(spec, &sentinel)
        .map_err(|_| internal("homebrew_qualification_stable_render"))?;
    let stable_parsed = parse_generated_formula(spec, &stable_projection, &stable_bytes)
        .map_err(|_| internal("homebrew_qualification_stable_parse"))?;
    if stable_channel.destination.is_empty()
        || stable_bytes.is_empty()
        || !stable_parsed.is_disabled_stable_sentinel()
    {
        return Err(internal("homebrew_qualification_stable_fixture"));
    }
    Ok(())
}

fn parse_record(
    bytes: &[u8],
    label: &str,
    plan: &velnor_actions_contract::PackageReleasePlan,
) -> Result<HistoricalRecord, OrchestratorError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| internal("homebrew_qualification_record_parse"))?;
    let strict = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| internal("homebrew_qualification_record_parse"))?;
    let record: HistoricalRecord = serde_json::from_value(strict)
        .map_err(|_| internal("homebrew_qualification_record_parse"))?;
    validate_record_identity(&record, label)?;
    validate_recovery_shape(&record, label)?;
    validate_record_release(&record, plan)?;
    Ok(record)
}

fn validate_record_identity(
    record: &HistoricalRecord,
    label: &str,
) -> Result<(), OrchestratorError> {
    if record.schema != "velnor.homebrew-historical-preview.v1"
        || record.label != label
        || record.repository.slug != REPOSITORY
        || record.repository.id != REPOSITORY_ID
        || record.plan_sha256 != PLAN_SHA256
        || record.release.receipt_name != "release.json"
        || !record.release.immutable
        || !record.release.prerelease
        || record.release.draft
        || record.source_ci.conclusion != "success"
        || record.source_ci.run_id == 0
        || record.source_ci.attempt == 0
        || record.producer.event != "workflow_run"
        || record.producer.run_id == 0
        || record.producer.attempt == 0
        || record.producer.workflow_path != ".github/workflows/binary-preview.yml"
        || record.producer.workflow_ref != "refs/heads/main"
        || record.producer.assemble_job.name != "preview-assemble"
        || record.producer.assemble_job.conclusion != "success"
        || record.producer.assemble_job.id == 0
        || record.producer.signer_job.name != "preview-signer"
        || record.producer.signer_job.conclusion != "success"
        || record.producer.signer_job.id == 0
        || record.producer.publisher_job.name != "Publish authenticated channel release"
        || record.producer.publisher_job.id == 0
        || record.release.version.is_empty()
        || record.release.receipt_size == 0
        || record.release.receipt_sha256.len() != 64
        || record.evidence.profile != "signed-sbom-v1"
        || record.evidence.issuer != "https://token.actions.githubusercontent.com"
        || record.evidence.predicate != "https://slsa.dev/provenance/v1"
        || record.evidence.signer_workflow
            != format!("{REPOSITORY}/.github/workflows/binary-preview.yml@refs/heads/main")
    {
        return Err(internal("homebrew_qualification_record_identity"));
    }
    Ok(())
}

fn validate_recovery_shape(
    record: &HistoricalRecord,
    label: &str,
) -> Result<(), OrchestratorError> {
    match (label, record.recovery.as_ref()) {
        ("A", Some(recovery))
            if record.producer.conclusion == "cancelled"
                && record.producer.publisher_job.conclusion == "cancelled"
                && recovery.event == "workflow_dispatch"
                && recovery.conclusion == "failure"
                && recovery.workflow_path == record.producer.workflow_path
                && recovery.workflow_ref == record.producer.workflow_ref
                && recovery.job.name
                    == "Recover the original signed release without rebuilding"
                && recovery.job.conclusion == "success"
                && recovery.job.id > 0
                && recovery.source_admission_artifact.entry == "preview-source-admission.json"
                && recovery.source_admission_artifact.source_sha == record.release.source_sha
                && recovery.source_admission_artifact.source_ci.run_id
                    == record.source_ci.run_id
                && recovery.source_admission_artifact.source_ci.attempt
                    == record.source_ci.attempt
                && recovery.source_admission_artifact.commit_count
                    == record.release.commit_count
                && recovery.source_admission_artifact.version == record.release.version => {}
        ("D", None)
            if record.producer.conclusion == "failure"
                && record.producer.publisher_job.conclusion == "success"
                && record.producer.failure_job_id.is_some_and(|id| id > 0)
                && record.producer.failure_scope.as_deref()
                    == Some("downstream-tap-preparation") => {}
        _ => return Err(internal("homebrew_qualification_historical_shape")),
    }
    Ok(())
}

fn validate_record_release(
    record: &HistoricalRecord,
    plan: &velnor_actions_contract::PackageReleasePlan,
) -> Result<(), OrchestratorError> {
    let expected_tag = format!("preview-{}", record.release.source_sha);
    if record.release.tag != expected_tag
        || record.release.source_sha != record.producer.authority_sha
        || plan.base_version.is_empty()
        || record.release.commit_count == 0
    {
        return Err(internal("homebrew_qualification_record_release"));
    }
    Ok(())
}

fn verify(
    policy: &crate::package_release_public::PackageReleasePublicPolicy,
    record: &HistoricalRecord,
    deadline: CheckDeadline,
) -> Result<crate::package_release_public::VerifiedPublicPackageRelease, OrchestratorError> {
    let selection = crate::package_release_public::PackageReleasePublicSelection::new(
        PackageReleaseChannel::Preview,
        record.release.tag.clone(),
        record.release.source_sha.clone(),
    )?;
    crate::package_release_public::verify_public_package_release(policy, &selection, deadline)
}

fn check_receipt(
    verified: &crate::package_release_public::VerifiedPublicPackageRelease,
    record: &HistoricalRecord,
) -> Result<(), OrchestratorError> {
    let receipt = verified.receipt();
    if receipt.channel != PackageReleaseChannel::Preview
        || receipt.repository.slug != REPOSITORY
        || receipt.repository.id != REPOSITORY_ID
        || receipt.source.commit_sha != record.release.source_sha
        || receipt.source.ci_run_id != record.source_ci.run_id
        || receipt.source.ci_run_attempt != record.source_ci.attempt
        || receipt.workflow.run_id != record.producer.run_id
        || receipt.workflow.run_attempt != record.producer.attempt
        || receipt.source.event.as_str() != record.producer.event
        || receipt.workflow.authority_sha != record.producer.authority_sha
        || receipt.tag != record.release.tag
        || receipt.version.value != record.release.version
        || receipt.version.commit_count != Some(record.release.commit_count)
    {
        return Err(internal("homebrew_qualification_receipt_expectation"));
    }
    let receipt_bytes = verified.asset_bytes("release.json")?;
    if receipt_bytes.len() as u64 != record.release.receipt_size
        || sha256(&receipt_bytes) != record.release.receipt_sha256
    {
        return Err(internal("homebrew_qualification_receipt_bytes"));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    let mut output = String::with_capacity(64);
    for byte in hash.finalize() {
        use std::fmt::Write as _;
        let _format_result = write!(output, "{byte:02x}");
    }
    output
}
