//! Validation for the current producer receipt before upload.

use crate::canonical::canonical_json_bytes;
use crate::errors::ContractError;
use crate::workflow::plan::Plan;
use crate::workflow::{QualificationPhase, QualificationRunRef};

use super::super::identity::configuration_commitment;
use super::admission::QualificationCacheAdmission;
use super::errors::invalid;
use super::types::{
    AdmissionNode, MAX_QUALIFICATION_RECEIPT_BYTES, QUALIFICATION_CACHE_RECEIPT_ARTIFACT,
    QualificationCacheArtifact, QualificationCacheReceiptArtifactDocument,
    QualificationCacheRunMetadata,
};
use super::validation::{validate_lanes, validate_receipt_shape};

impl QualificationCacheReceiptArtifactDocument {
    /// Validate and serialize one bounded immutable producer artifact.
    /// # Errors
    pub fn to_bounded_json(
        &self,
        plan: &Plan,
        metadata: &QualificationCacheRunMetadata,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<Vec<u8>, ContractError> {
        self.validate_for_plan(plan, metadata, admission)?;
        let bytes = canonical_json_bytes(self)?;
        if bytes.len() > MAX_QUALIFICATION_RECEIPT_BYTES {
            return Err(invalid("artifact_size"));
        }
        Ok(bytes)
    }

    /// Validate completed producer evidence before the collector uploads it.
    /// # Errors
    pub fn validate_for_plan(
        &self,
        plan: &Plan,
        metadata: &QualificationCacheRunMetadata,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<(), ContractError> {
        plan.validate()?;
        let context = plan
            .qualification
            .as_ref()
            .ok_or_else(|| invalid("missing_dispatch"))?;
        let current_run = current_run(context);
        self.validate_binding(plan, context, metadata, current_run)?;
        self.validate_predecessor(plan, context.phase, admission)?;
        validate_receipt_shape(&self.receipt)?;
        self.validate_lanes(plan, context, metadata, admission, current_run)
    }

    fn validate_binding(
        &self,
        plan: &Plan,
        context: &crate::workflow::QualificationDispatch,
        metadata: &QualificationCacheRunMetadata,
        current_run: QualificationRunRef,
    ) -> Result<(), ContractError> {
        let branch = context
            .git_ref
            .strip_prefix("refs/heads/")
            .filter(|branch| crate::is_valid_branch_name(branch))
            .ok_or_else(|| invalid("completed_dispatch_ref_invalid"))?;
        let workflow_path = context
            .workflow_ref
            .strip_prefix(&format!("{}/", context.repository))
            .and_then(|suffix| suffix.split_once('@').map(|(path, _)| path))
            .filter(|path| {
                *path == crate::workflow::CI_WORKFLOW_PATH
                    || *path == crate::workflow::QUALIFICATION_WORKFLOW_PATH
            })
            .ok_or_else(|| invalid("completed_dispatch_workflow_invalid"))?;
        if self.schema != 2
            || self.producer.repository != context.repository
            || self.producer.default_branch != context.default_branch
            || self.producer.git_ref != context.git_ref
            || self.producer.ref_protected != context.ref_protected
            || self.producer.workflow_ref != context.workflow_ref
            || self.producer.workflow_sha != context.workflow_sha
            || self.producer.workflow_sha != context.source_sha
            || self.producer.source_sha != context.source_sha
            || self.producer.run != current_run
            || metadata.run != current_run
            || metadata.repository != context.repository
            || metadata.default_branch != context.default_branch
            || metadata.git_ref != context.git_ref
            || metadata.ref_protected != context.ref_protected
            || metadata.workflow_ref != self.producer.workflow_ref
            || metadata.workflow_sha != self.producer.workflow_sha
            || metadata.head_sha != context.source_sha
            || metadata.event != "workflow_dispatch"
            || metadata.workflow_path_ref != format!("{workflow_path}@{branch}")
            || metadata.workflow_ref != context.workflow_ref
            || self.receipt.plan_id != plan.plan_id
            || self.receipt.run != current_run
            || self.receipt.campaign != context.campaign
            || self.receipt.phase != context.phase
            || self.receipt.source_sha != context.source_sha
            || self.receipt.configuration_digest != configuration_commitment(plan)?
        {
            return Err(invalid("completed_receipt_binding_mismatch"));
        }
        Ok(())
    }

    fn validate_predecessor(
        &self,
        plan: &Plan,
        phase: QualificationPhase,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<(), ContractError> {
        match (phase.predecessor(), admission) {
            (None, None) if self.receipt.predecessor.is_none() => {}
            (Some(_), Some(previous)) => {
                previous.validate_for_plan(plan)?;
                let link = self
                    .receipt
                    .predecessor
                    .as_ref()
                    .ok_or_else(|| invalid("completed_receipt_missing_predecessor"))?;
                if link.run != previous.receipt().run
                    || link.receipt_digest != previous.receipt_digest()?
                    || link.artifact_id != previous.root.artifact.id
                    || link.artifact_digest != previous.root.artifact.digest
                {
                    return Err(invalid("completed_receipt_predecessor_mismatch"));
                }
                if phase == QualificationPhase::UsefulDelta
                    && self.receipt.source_delta != previous.source_delta().cloned()
                {
                    return Err(invalid("completed_source_delta_mismatch"));
                }
            }
            _ => return Err(invalid("completed_receipt_predecessor_shape")),
        }
        Ok(())
    }

    fn validate_lanes(
        &self,
        plan: &Plan,
        context: &crate::workflow::QualificationDispatch,
        metadata: &QualificationCacheRunMetadata,
        admission: Option<&QualificationCacheAdmission>,
        current_run: QualificationRunRef,
    ) -> Result<(), ContractError> {
        let branch = context
            .git_ref
            .strip_prefix("refs/heads/")
            .filter(|branch| crate::is_valid_branch_name(branch))
            .ok_or_else(|| invalid("completed_dispatch_ref_invalid"))?;
        let artifact = QualificationCacheArtifact {
            id: 1,
            name: QUALIFICATION_CACHE_RECEIPT_ARTIFACT.to_owned(),
            digest: format!("sha256:{}", "0".repeat(64)),
            size_bytes: 1,
            expired: false,
            workflow_run_id: current_run.run_id,
            workflow_head_branch: branch.to_owned(),
            workflow_head_sha: context.source_sha.clone(),
        };
        let node = AdmissionNode {
            metadata: metadata.clone(),
            artifact,
            producer: self.producer.clone(),
            receipt: self.receipt.clone(),
            previous: admission.map(|value| value.root.clone()),
        };
        validate_lanes(plan, &node, admission.map(|value| value.root.as_ref()))
    }
}

fn current_run(context: &crate::workflow::QualificationDispatch) -> QualificationRunRef {
    QualificationRunRef {
        run_id: context.run_id,
        run_attempt: context.run_attempt,
    }
}
