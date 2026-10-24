use crate::OrchestratorError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HistoricalRecord {
    pub(super) schema: String,
    pub(super) label: String,
    pub(super) repository: Repository,
    pub(super) plan_sha256: String,
    pub(super) release: Release,
    pub(super) source_ci: CiRun,
    pub(super) producer: Producer,
    pub(super) recovery: Option<Recovery>,
    pub(super) evidence: Evidence,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Repository {
    pub(super) slug: String,
    pub(super) id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Release {
    pub(super) id: u64,
    pub(super) tag: String,
    pub(super) source_sha: String,
    pub(super) version: String,
    pub(super) commit_count: u64,
    pub(super) receipt_asset_id: u64,
    pub(super) receipt_name: String,
    pub(super) receipt_size: u64,
    pub(super) receipt_sha256: String,
    pub(super) immutable: bool,
    pub(super) prerelease: bool,
    pub(super) draft: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CiRun {
    pub(super) run_id: u64,
    pub(super) attempt: u32,
    pub(super) conclusion: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Producer {
    pub(super) run_id: u64,
    pub(super) attempt: u32,
    pub(super) event: String,
    pub(super) conclusion: String,
    pub(super) authority_sha: String,
    pub(super) workflow_path: String,
    pub(super) workflow_ref: String,
    pub(super) assemble_job: Job,
    pub(super) signer_job: Job,
    pub(super) publisher_job: Job,
    pub(super) failure_job_id: Option<u64>,
    pub(super) failure_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Job {
    pub(super) name: String,
    pub(super) id: u64,
    pub(super) conclusion: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recovery {
    pub(super) run_id: u64,
    pub(super) attempt: u32,
    pub(super) event: String,
    pub(super) conclusion: String,
    pub(super) authority_sha: String,
    pub(super) workflow_path: String,
    pub(super) workflow_ref: String,
    pub(super) job: Job,
    pub(super) source_admission_artifact: AdmissionArtifact,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionArtifact {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) entry: String,
    pub(super) sha256: String,
    pub(super) policy_sha256: String,
    pub(super) source_sha: String,
    pub(super) source_ci: RecoveryCi,
    pub(super) commit_count: u64,
    pub(super) version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryCi {
    pub(super) run_id: u64,
    pub(super) attempt: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    pub(super) profile: String,
    pub(super) issuer: String,
    pub(super) predicate: String,
    pub(super) signer_workflow: String,
}

pub(super) fn admission(
    record: &HistoricalRecord,
    plan: &velnor_actions_contract::PackageReleasePlan,
    durable_admission: Option<&[u8]>,
) -> Result<crate::package_release_public::HistoricalPackageReleaseAdmission, OrchestratorError> {
    let mut admission = crate::package_release_public::HistoricalPackageReleaseAdmission::new_from_protected_policy(
        plan, record.release.source_sha.clone(), record.release.tag.clone(), record.release.id,
        record.release.receipt_asset_id, record.release.receipt_sha256.clone(), record.plan_sha256.clone(),
    )?;
    if let Some(recovery) = &record.recovery {
        let admission_bytes = durable_admission
            .ok_or_else(|| crate::internal::internal("homebrew_qualification_admission_missing"))?;
        let artifact = &recovery.source_admission_artifact;
        let recovery = crate::package_release_public::HistoricalPackageReleaseRecovery::new_from_protected_policy(
            crate::package_release_public::HistoricalPackageReleaseRecoveryInput {
                original_publisher_job_id: record.producer.publisher_job.id,
                run_id: recovery.run_id,
                run_attempt: recovery.attempt,
                authority_sha: recovery.authority_sha.clone(),
                recovery_job_id: recovery.job.id,
                artifact_id: artifact.id,
                artifact_name: artifact.name.clone(),
                admission_size: artifact.size,
                admission_sha256: artifact.sha256.clone(),
                policy_sha256: artifact.policy_sha256.clone(),
                admission_bytes: admission_bytes.to_vec(),
            },
        )?;
        admission = admission.with_recovery_admission(recovery)?;
    } else if durable_admission.is_some() {
        return Err(crate::internal::internal(
            "homebrew_qualification_admission_unexpected",
        ));
    }
    Ok(admission)
}
