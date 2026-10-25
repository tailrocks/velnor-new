//! Producer evidence checks and completed-run receipt creation.

use crate::canonical::canonical_json_bytes;
use crate::errors::ContractError;
use crate::workflow::plan::Plan;
use crate::workflow::{
    QualificationCacheReceipt, QualificationCacheRunMetadata, QualificationPhase,
    QualificationRunRef,
};

use super::super::identity::configuration_commitment;
use super::admission::QualificationCacheAdmission;
use super::errors::invalid;
use super::types::{MAX_QUALIFICATION_RECEIPT_BYTES, QualificationCacheReceiptArtifactDocument};
use super::validation::{LaneEvidence, validate_lanes, validate_producer_evidence_shape};

impl QualificationCacheReceiptArtifactDocument {
    /// Make sure producer evidence agrees with the plan before writing it.
    /// # Errors
    pub fn to_bounded_json(
        &self,
        plan: &Plan,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<Vec<u8>, ContractError> {
        self.validate_producer_for_plan(plan, admission)?;
        let bytes = canonical_json_bytes(self)?;
        if bytes.len() > MAX_QUALIFICATION_RECEIPT_BYTES {
            return Err(invalid("artifact_size"));
        }
        Ok(bytes)
    }

    /// Make sure in-run observations agree with this plan and phase.
    /// # Errors
    pub fn validate_producer_for_plan(
        &self,
        plan: &Plan,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<(), ContractError> {
        plan.validate()?;
        let context = plan
            .qualification
            .as_ref()
            .ok_or_else(|| invalid("missing_dispatch"))?;
        self.validate_producer_context(plan, context)?;
        self.validate_predecessor(plan, context.phase, admission)?;
        validate_producer_evidence_shape(&self.evidence, &self.producer)?;
        validate_lanes(
            plan,
            &self.producer,
            LaneEvidence::producer(&self.evidence),
            admission.map(|value| value.root.as_ref()),
        )
    }

    /// Compare GitHub's completed-run data with the producer facts.
    /// # Errors
    pub fn validate_for_plan(
        &self,
        plan: &Plan,
        metadata: &QualificationCacheRunMetadata,
        admission: Option<&QualificationCacheAdmission>,
    ) -> Result<(), ContractError> {
        self.validate_producer_for_plan(plan, admission)?;
        self.validate_completion_metadata(metadata)
    }

    /// Make the completed receipt only after GitHub reports success for this run.
    /// # Errors
    pub fn completed_receipt(
        &self,
        metadata: &QualificationCacheRunMetadata,
    ) -> Result<QualificationCacheReceipt, ContractError> {
        self.validate_completion_metadata(metadata)?;
        if self.evidence.source_sha != self.producer.source_sha {
            return Err(invalid("producer_evidence_binding_mismatch"));
        }
        validate_producer_evidence_shape(&self.evidence, &self.producer)?;
        let receipt = QualificationCacheReceipt {
            schema: 4,
            plan_id: self.evidence.plan_id.clone(),
            run: self.producer.run,
            campaign: self.evidence.campaign.clone(),
            phase: self.evidence.phase,
            source_sha: self.evidence.source_sha.clone(),
            configuration_digest: self.evidence.configuration_digest.clone(),
            source_delta: self.evidence.source_delta.clone(),
            predecessor: self.evidence.predecessor.clone(),
            lanes: self.evidence.lanes.clone(),
            owners: self.evidence.owners.clone(),
        };
        super::validation::validate_receipt_shape(&receipt)?;
        Ok(receipt)
    }

    fn validate_producer_context(
        &self,
        plan: &Plan,
        context: &crate::workflow::QualificationDispatch,
    ) -> Result<(), ContractError> {
        let expected_run = current_run(context);
        let branch = context
            .git_ref
            .strip_prefix("refs/heads/")
            .filter(|branch| crate::is_valid_branch_name(branch))
            .ok_or_else(|| invalid("producer_dispatch_ref_invalid"))?;
        workflow_path(&context.workflow_ref, &context.repository)?;
        if self.schema != 4
            || self.producer.mode != context.mode
            || self.producer.repository != context.repository
            || self.producer.default_branch != context.default_branch
            || self.producer.git_ref != context.git_ref
            || self.producer.ref_protected != context.ref_protected
            || self.producer.workflow_ref != context.workflow_ref
            || self.producer.workflow_sha != context.workflow_sha
            || self.producer.workflow_sha != context.source_sha
            || self.producer.source_sha != context.source_sha
            || self.producer.run != expected_run
            || self.evidence.plan_id != plan.plan_id
            || self.evidence.campaign != context.campaign
            || self.evidence.phase != context.phase
            || self.evidence.source_sha != context.source_sha
            || self.evidence.configuration_digest != configuration_commitment(plan)?
            || self.producer.git_ref != format!("refs/heads/{branch}")
        {
            return Err(invalid("producer_evidence_binding_mismatch"));
        }
        Ok(())
    }

    fn validate_completion_metadata(
        &self,
        metadata: &QualificationCacheRunMetadata,
    ) -> Result<(), ContractError> {
        let branch = self
            .producer
            .git_ref
            .strip_prefix("refs/heads/")
            .filter(|branch| crate::is_valid_branch_name(branch))
            .ok_or_else(|| invalid("producer_dispatch_ref_invalid"))?;
        let workflow = workflow_path(&self.producer.workflow_ref, &self.producer.repository)?;
        if self.schema != 4
            || metadata.conclusion != "success"
            || metadata.event != "workflow_dispatch"
            || metadata.run != self.producer.run
            || metadata.repository != self.producer.repository
            || metadata.default_branch != self.producer.default_branch
            || metadata.git_ref != self.producer.git_ref
            || metadata.ref_protected != self.producer.ref_protected
            || metadata.workflow_path_ref != format!("{workflow}@{branch}")
            || metadata.workflow_ref != self.producer.workflow_ref
            || metadata.workflow_sha != self.producer.workflow_sha
            || metadata.head_sha != self.producer.source_sha
            || self.producer.workflow_sha != self.producer.source_sha
        {
            return Err(invalid("completed_run_metadata_mismatch"));
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
            (None, None) if self.evidence.predecessor.is_none() => {}
            (Some(_), Some(previous)) => {
                previous.validate_for_plan(plan)?;
                let link = self
                    .evidence
                    .predecessor
                    .as_ref()
                    .ok_or_else(|| invalid("producer_evidence_missing_predecessor"))?;
                if link.run != previous.receipt().run
                    || link.receipt_digest != previous.receipt_digest()?
                    || link.artifact_id != previous.root.artifact.id
                    || link.artifact_digest != previous.root.artifact.digest
                {
                    return Err(invalid("producer_evidence_predecessor_mismatch"));
                }
                if phase == QualificationPhase::UsefulDelta
                    && self.evidence.source_delta != previous.source_delta().cloned()
                {
                    return Err(invalid("producer_source_delta_mismatch"));
                }
            }
            _ => return Err(invalid("producer_evidence_predecessor_shape")),
        }
        Ok(())
    }
}

fn workflow_path<'a>(workflow_ref: &'a str, repository: &str) -> Result<&'a str, ContractError> {
    workflow_ref
        .strip_prefix(&format!("{repository}/"))
        .and_then(|suffix| suffix.split_once('@').map(|(path, _)| path))
        .filter(|path| {
            *path == crate::workflow::CI_WORKFLOW_PATH
                || *path == crate::workflow::QUALIFICATION_WORKFLOW_PATH
        })
        .ok_or_else(|| invalid("producer_workflow_invalid"))
}

fn current_run(context: &crate::workflow::QualificationDispatch) -> QualificationRunRef {
    QualificationRunRef {
        run_id: context.run_id,
        run_attempt: context.run_attempt,
    }
}
