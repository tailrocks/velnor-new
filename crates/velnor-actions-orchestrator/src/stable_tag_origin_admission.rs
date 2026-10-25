use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, parse_package_release_plan,
    parse_package_release_policy, preview_production::ReleaseProductionContext,
};
use velnor_actions_mise::CheckDeadline;

use super::super::github::GitHubApi;
use super::{
    ArtifactRef, Authorization, artifact_ref, artifacts, authenticate_current, canonical,
    current_authorization, fixed_path, observe_tag, plan, required, sha256, write_new,
};
use crate::{OrchestratorError, internal::internal};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    intent: Authorization,
    origin_archive: ArtifactRef,
    intent_archive: ArtifactRef,
}

pub fn admit_stable_tag_origin_from_env() -> Result<(), OrchestratorError> {
    super::super::artifact_zip::within_operation(admit_origin)
}

fn admit_origin() -> Result<(), OrchestratorError> {
    if required("VELNOR_INTERNAL_OP")? != super::ADMIT_STABLE_TAG_ORIGIN_OP {
        return Err(internal("stable_tag_admission_operation"));
    }
    let plan = plan()?;
    let expected = current_authorization(&plan)?;
    let api = GitHubApi::new(plan.repository.clone());
    authenticate_current(&api, &plan, &expected.context, false)?;
    let jobs = api.complete_workflow_jobs(expected.context.run_id, expected.context.run_attempt)?;
    let tag_jobs: Vec<_> = jobs
        .jobs
        .iter()
        .filter(|job| job.name == "Authorize the permanent stable tag")
        .collect();
    if tag_jobs.len() != 1
        || tag_jobs[0].run_id != expected.context.run_id
        || tag_jobs[0].head_sha.as_deref() != Some(expected.context.source_sha.as_str())
        || tag_jobs[0].status.as_deref() != Some("completed")
        || tag_jobs[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("stable_tag_origin_job_unsuccessful"));
    }
    let directory = fixed_path("admission.json")?
        .parent()
        .ok_or_else(|| internal("stable_tag_temp"))?
        .to_owned();
    super::ensure_directory(&directory)?;
    let origin_archive = artifact_ref("VELNOR_STABLE_TAG_ORIGIN_ARTIFACT")?;
    let intent_archive = artifact_ref("VELNOR_STABLE_TAG_INTENT_ARTIFACT")?;
    let origin = artifacts::origin(&api, &expected, &origin_archive)?;
    if origin.intent != intent_archive
        || origin.tag != observe_tag(&api, &plan)?
        || origin.tag.target_sha != expected.context.source_sha
    {
        return Err(internal("stable_tag_origin_tag_changed"));
    }
    let original = artifacts::intent(&api, &origin.authorization, &intent_archive)?;
    artifacts::require_intent_object(&origin, &original)?;
    let admission = Admission {
        schema: "velnor.stable-production-admission.v1".to_owned(),
        intent: expected,
        origin_archive,
        intent_archive,
    };
    let bytes = canonical(&admission)?;
    super::io::write_resolved_plan(
        &plan
            .canonical_bytes()
            .map_err(|_| internal("stable_production_plan"))?,
    )?;
    write_new("admission.json", &bytes)?;
    append_output(&format!("admission_digest={}\n", sha256(&bytes)))
}

pub(crate) fn admitted_policy_plan(
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(PackageReleasePolicy, PackageReleasePlan), OrchestratorError> {
    deadline
        .scope(|| {
            let (policy, plan, _) = read_admission(temp)?;
            Ok((policy, plan))
        })
        .map_err(|_| internal("stable_production_admission_deadline"))?
}

pub(crate) fn admitted_production_context(
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<ReleaseProductionContext, OrchestratorError> {
    deadline
        .scope(|| {
            let temp = std::path::PathBuf::from(required("RUNNER_TEMP")?).join("velnor");
            let (_, resolved, admission) = read_admission(&temp)?;
            if resolved != *plan {
                return Err(internal("stable_production_plan_substitution"));
            }
            Ok(admission.intent.context)
        })
        .map_err(|_| internal("stable_production_admission_deadline"))?
}

fn read_admission(
    temp: &Path,
) -> Result<(PackageReleasePolicy, PackageReleasePlan, Admission), OrchestratorError> {
    if !temp.is_absolute()
        || temp != std::path::PathBuf::from(required("RUNNER_TEMP")?).join("velnor")
    {
        return Err(internal("stable_production_admission_path"));
    }
    let bytes = super::read_metadata(&temp.join("stable-tag/admission.json"))?;
    if sha256(&bytes) != required("VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST")? {
        return Err(internal("stable_production_admission_digest"));
    }
    let admission: Admission = artifacts::decode(&bytes)?;
    let policy_bytes = required("VELNOR_RELEASE_POLICY")?;
    let policy = parse_package_release_policy(policy_bytes.as_bytes())
        .map_err(|_| internal("stable_production_policy"))?;
    let plan_bytes = super::read_metadata(&temp.join("package-release-plan.json"))?;
    let plan =
        parse_package_release_plan(&plan_bytes).map_err(|_| internal("stable_production_plan"))?;
    policy
        .validate_plan(&plan)
        .map_err(|_| internal("stable_production_policy_plan"))?;
    if admission.schema != "velnor.stable-production-admission.v1"
        || admission.intent.policy_sha256 != sha256(policy_bytes.as_bytes())
        || admission.intent.plan_sha256 != sha256(&plan_bytes)
        || plan
            .canonical_bytes()
            .map_err(|_| internal("stable_production_plan"))?
            != plan_bytes
        || admission.intent.context != super::current_context(&plan)?
        || admission.intent.version_package != policy.version.version_package
        || admission.origin_archive != artifact_ref("VELNOR_STABLE_TAG_ORIGIN_ARTIFACT")?
        || admission.intent_archive != artifact_ref("VELNOR_STABLE_TAG_INTENT_ARTIFACT")?
    {
        return Err(internal("stable_production_admission_context"));
    }
    Ok((policy, plan, admission))
}

fn append_output(value: &str) -> Result<(), OrchestratorError> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_OUTPUT")?)
        .map_err(|_| internal("stable_tag_output_open"))?;
    file.write_all(value.as_bytes())
        .map_err(|_| internal("stable_tag_output_append"))
}
