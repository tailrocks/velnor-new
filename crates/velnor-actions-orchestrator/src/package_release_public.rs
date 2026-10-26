// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use std::time::Duration;

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
};

use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};

const PUBLIC_VERIFY_BUDGET: Duration = Duration::from_mins(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HistoricalPackageReleaseAdmission {
    source_sha: String,
    tag: String,
    release_id: u64,
    receipt_asset_id: u64,
    receipt_sha256: String,
    plan_sha256: String,
    recovery: Option<HistoricalPackageReleaseRecovery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HistoricalPackageReleaseRecovery {
    pub(super) original_publisher_job_id: u64,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
    pub(super) authority_sha: String,
    pub(super) recovery_job_id: u64,
    pub(super) artifact_id: u64,
    pub(super) artifact_name: String,
    pub(super) admission_size: u64,
    pub(super) admission_sha256: String,
    pub(super) policy_sha256: String,
    pub(super) admission_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoricalPackageReleaseRecoveryInput {
    pub(crate) original_publisher_job_id: u64,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) authority_sha: String,
    pub(crate) recovery_job_id: u64,
    pub(crate) artifact_id: u64,
    pub(crate) artifact_name: String,
    pub(crate) admission_size: u64,
    pub(crate) admission_sha256: String,
    pub(crate) policy_sha256: String,
    pub(crate) admission_bytes: Vec<u8>,
}

impl HistoricalPackageReleaseRecovery {
    pub(crate) fn new_from_protected_policy(
        input: HistoricalPackageReleaseRecoveryInput,
    ) -> Result<Self, OrchestratorError> {
        if input.original_publisher_job_id == 0
            || input.run_id == 0
            || input.run_attempt == 0
            || !is_full_sha(&input.authority_sha)
            || input.recovery_job_id == 0
            || input.artifact_id == 0
            || input.artifact_name
                != format!(
                    "velnor-preview-source-r{}-a{}",
                    input.run_id, input.run_attempt
                )
            || input.admission_size == 0
            || input.admission_size > 16 * 1024 * 1024
            || u64::try_from(input.admission_bytes.len()).ok() != Some(input.admission_size)
            || !is_sha256(&input.admission_sha256)
            || sha256(&input.admission_bytes) != input.admission_sha256
            || !is_sha256(&input.policy_sha256)
        {
            return Err(internal("package_public_historical_recovery_invalid"));
        }
        Ok(Self {
            original_publisher_job_id: input.original_publisher_job_id,
            run_id: input.run_id,
            run_attempt: input.run_attempt,
            authority_sha: input.authority_sha,
            recovery_job_id: input.recovery_job_id,
            artifact_id: input.artifact_id,
            artifact_name: input.artifact_name,
            admission_size: input.admission_size,
            admission_sha256: input.admission_sha256,
            policy_sha256: input.policy_sha256,
            admission_bytes: input.admission_bytes,
        })
    }
}

impl HistoricalPackageReleaseAdmission {
    pub(crate) fn new_from_protected_policy(
        plan: &PackageReleasePlan,
        source_sha: String,
        tag: String,
        release_id: u64,
        receipt_asset_id: u64,
        receipt_sha256: String,
        plan_sha256: String,
    ) -> Result<Self, OrchestratorError> {
        plan.validate()
            .map_err(|_| internal("package_public_plan_invalid"))?;
        if release_id == 0
            || receipt_asset_id == 0
            || !is_full_sha(&source_sha)
            || !is_sha256(&receipt_sha256)
            || !is_sha256(&plan_sha256)
            || sha256(
                &plan
                    .canonical_bytes()
                    .map_err(|_| internal("package_public_plan_invalid"))?,
            ) != plan_sha256
            || tag_for(plan, &source_sha)? != tag
        {
            return Err(internal("package_public_historical_admission_invalid"));
        }
        Ok(Self {
            source_sha,
            tag,
            release_id,
            receipt_asset_id,
            receipt_sha256,
            plan_sha256,
            recovery: None,
        })
    }

    pub(crate) fn with_recovery_admission(
        mut self,
        recovery: HistoricalPackageReleaseRecovery,
    ) -> Result<Self, OrchestratorError> {
        if self.recovery.is_some()
            || recovery.original_publisher_job_id == 0
            || recovery.run_id == 0
            || recovery.run_attempt == 0
            || !is_full_sha(&recovery.authority_sha)
            || recovery.recovery_job_id == 0
            || recovery.artifact_id == 0
            || recovery.artifact_name
                != format!(
                    "velnor-preview-source-r{}-a{}",
                    recovery.run_id, recovery.run_attempt
                )
            || !is_sha256(&recovery.admission_sha256)
            || u64::try_from(recovery.admission_bytes.len()).ok() != Some(recovery.admission_size)
            || sha256(&recovery.admission_bytes) != recovery.admission_sha256
            || !is_sha256(&recovery.policy_sha256)
        {
            return Err(internal("package_public_historical_recovery_invalid"));
        }
        self.recovery = Some(recovery);
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PackageReleasePublicPolicy {
    plan: PackageReleasePlan,
    repository_id: u64,
    authority_sha: String,
    historical: Vec<HistoricalPackageReleaseAdmission>,
}

impl PackageReleasePublicPolicy {
    pub(crate) fn from_trusted_source_authority(
        plan: PackageReleasePlan,
        repository_id: u64,
        authority_sha: String,
    ) -> Result<Self, OrchestratorError> {
        plan.validate()
            .map_err(|_| internal("package_public_plan_invalid"))?;
        if repository_id == 0 || !is_full_sha(&authority_sha) {
            return Err(internal("package_public_authority_invalid"));
        }
        Ok(Self {
            plan,
            repository_id,
            authority_sha,
            historical: Vec::new(),
        })
    }

    pub(crate) fn with_historical_admissions(
        mut self,
        admissions: Vec<HistoricalPackageReleaseAdmission>,
    ) -> Result<Self, OrchestratorError> {
        let mut tags = std::collections::BTreeSet::new();
        let expected_plan = sha256(
            &self
                .plan
                .canonical_bytes()
                .map_err(|_| internal("package_public_plan_invalid"))?,
        );
        if admissions.iter().any(|admission| {
            !tags.insert(admission.tag.clone()) || admission.plan_sha256 != expected_plan
        }) {
            return Err(internal("package_public_historical_admission_collision"));
        }
        self.historical = admissions;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageReleasePublicSelection {
    channel: PackageReleaseChannel,
    tag: String,
    source_sha: String,
}

impl PackageReleasePublicSelection {
    pub(crate) fn new(
        channel: PackageReleaseChannel,
        tag: String,
        source_sha: String,
    ) -> Result<Self, OrchestratorError> {
        if !is_full_sha(&source_sha) {
            return Err(internal("package_public_source_sha_invalid"));
        }
        Ok(Self {
            channel,
            tag,
            source_sha,
        })
    }
}

#[derive(Debug)]
pub(crate) struct VerifiedPublicPackageRelease {
    plan: PackageReleasePlan,
    receipt: PackageReleaseReceipt,
    signed: VerifiedAttestedSignedPackage,
    historical: bool,
    authority_sha: String,
}

impl VerifiedPublicPackageRelease {
    pub(crate) fn plan(&self) -> &PackageReleasePlan {
        &self.plan
    }

    pub(crate) fn receipt(&self) -> &PackageReleaseReceipt {
        &self.receipt
    }

    pub(crate) fn asset_bytes(&self, name: &str) -> Result<Vec<u8>, OrchestratorError> {
        if name != "release.json"
            && name != "release.json.bundle"
            && !self.receipt.assets.iter().any(|asset| asset.name == name)
        {
            return Err(internal("package_public_asset_not_admitted"));
        }
        self.signed
            .closure()
            .read_file(name, 1024 * 1024 * 1024)
            .map_err(|_| internal("package_public_asset_read_failed"))
    }

    pub(crate) fn is_historical_admission(&self) -> bool {
        self.historical
    }

    pub(crate) fn authority_sha(&self) -> &str {
        &self.authority_sha
    }
}

#[path = "package_release_public_assets.rs"]
mod assets;
#[path = "package_release_public_verify.rs"]
mod verifier;

use assets::{is_full_sha, is_sha256, sha256, tag_for};
pub(crate) use verifier::verify_public_package_release;
