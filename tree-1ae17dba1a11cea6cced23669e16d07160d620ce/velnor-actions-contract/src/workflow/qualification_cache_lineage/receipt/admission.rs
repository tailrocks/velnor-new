//! Bounded predecessor admission and lineage validation.

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::plan::Plan;
use crate::workflow::{QualificationDispatch, QualificationPhase, QualificationRunRef};

use super::super::identity::configuration_commitment;
use super::super::identity::{QualificationCacheLayer, QualificationCacheSlot};
use super::errors::{check_json_nesting, invalid, receipt_digest, run_ref_before};
use super::lineage::{find_latest_saved_entry, find_layer_receipt, find_saved_entry};
use super::source_delta::validate_source_delta;
use super::types::{
    AdmissionDocument, AdmissionNode, MAX_QUALIFICATION_RECEIPT_BYTES,
    MAX_QUALIFICATION_RECEIPT_DEPTH, QualificationCacheBackendEntry,
    QualificationCacheLayerReceipt, QualificationCacheReceipt, QualificationSourceDelta,
};
use super::validation::{
    validate_lanes, validate_metadata, validate_node_metadata, validate_receipt_shape,
};

/// Validated, bounded predecessor receipt and its complete lineage.
#[derive(Debug, Clone)]
pub struct QualificationCacheAdmission {
    pub(super) root: Box<AdmissionNode>,
    pub(super) source_delta: Option<QualificationSourceDelta>,
}

impl QualificationCacheAdmission {
    /// Parse a staged predecessor document after byte/depth limits pass.
    /// # Errors
    pub fn parse_bounded(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.is_empty() || bytes.len() > MAX_QUALIFICATION_RECEIPT_BYTES {
            return Err(invalid("artifact_size"));
        }
        check_json_nesting(bytes)?;
        let document: AdmissionDocument =
            serde_json::from_slice(bytes).map_err(|_| invalid("malformed_admission"))?;
        let admission = Self {
            root: Box::new(document.predecessor),
            source_delta: document.source_delta,
        };
        admission.validate_shape()?;
        Ok(admission)
    }

    /// Validate receipt metadata and full predecessor lineage for a plan.
    /// # Errors
    pub fn validate_for_plan(&self, plan: &Plan) -> Result<(), ContractError> {
        plan.validate()?;
        let dispatch = plan
            .qualification
            .as_ref()
            .ok_or_else(|| invalid("missing_dispatch"))?;
        let expected_phase = dispatch
            .phase
            .predecessor()
            .ok_or_else(|| invalid("phase_has_no_predecessor"))?;
        let expected_run = dispatch
            .predecessor
            .ok_or_else(|| invalid("missing_predecessor_reference"))?;
        if self.root.receipt.run != expected_run {
            return Err(invalid("predecessor_reference_mismatch"));
        }
        let current = QualificationRunRef {
            run_id: dispatch.run_id,
            run_attempt: dispatch.run_attempt,
        };
        if !run_ref_before(expected_run, current) {
            return Err(invalid("predecessor_run_not_before_current"));
        }
        let current_config = configuration_commitment(plan)?;
        self.validate_source_delta(dispatch)?;
        Self::validate_chain(plan, &self.root, expected_phase, &current_config, 1)
    }

    /// Canonical digest of the immediate predecessor receipt.
    /// # Errors
    pub fn receipt_digest(&self) -> Result<String, ContractError> {
        Ok(digest_b3(&canonical_json_bytes(&self.root.receipt)?))
    }

    pub(crate) fn receipt(&self) -> &QualificationCacheReceipt {
        &self.root.receipt
    }

    pub(crate) fn layer_receipt(
        &self,
        matrix_key: &str,
        layer: QualificationCacheLayer,
    ) -> Result<&QualificationCacheLayerReceipt, ContractError> {
        find_layer_receipt(Some(&self.root), matrix_key, layer)
    }

    pub(crate) fn saved_entry(
        &self,
        matrix_key: &str,
        layer: QualificationCacheLayer,
        slot: QualificationCacheSlot,
    ) -> Result<QualificationCacheBackendEntry, ContractError> {
        find_saved_entry(Some(&self.root), matrix_key, layer, slot)
    }

    pub(crate) fn latest_saved_entry(
        &self,
        matrix_key: &str,
        layer: QualificationCacheLayer,
    ) -> Result<(QualificationCacheSlot, QualificationCacheBackendEntry), ContractError> {
        find_latest_saved_entry(Some(&self.root), matrix_key, layer)
    }

    /// Verified source change for `UsefulDelta`; absent in other phases.
    #[must_use]
    pub fn source_delta(&self) -> Option<&QualificationSourceDelta> {
        self.source_delta.as_ref()
    }

    fn validate_source_delta(&self, dispatch: &QualificationDispatch) -> Result<(), ContractError> {
        match (dispatch.phase, self.source_delta.as_ref()) {
            (QualificationPhase::UsefulDelta, Some(delta)) => {
                validate_source_delta(delta, &self.root.receipt.source_sha, &dispatch.source_sha)
            }
            (QualificationPhase::UsefulDelta, None) => Err(invalid("missing_useful_source_delta")),
            (_, None) if self.root.receipt.source_sha == dispatch.source_sha => Ok(()),
            _ => Err(invalid("unexpected_or_unbound_source_delta")),
        }
    }

    fn validate_shape(&self) -> Result<(), ContractError> {
        Self::validate_node_shape(&self.root, 1)
    }

    fn validate_node_shape(node: &AdmissionNode, depth: usize) -> Result<(), ContractError> {
        if depth >= MAX_QUALIFICATION_RECEIPT_DEPTH {
            return Err(invalid("lineage_too_deep"));
        }
        validate_metadata(node)?;
        validate_receipt_shape(&node.receipt)?;
        if let Some(previous) = &node.previous {
            let link = node
                .receipt
                .predecessor
                .as_ref()
                .ok_or_else(|| invalid("missing_predecessor_link"))?;
            if link.run != previous.receipt.run
                || link.receipt_digest != receipt_digest(&previous.receipt)?
                || link.artifact_id != previous.artifact.id
                || link.artifact_digest != previous.artifact.digest
            {
                return Err(invalid("predecessor_receipt_digest_mismatch"));
            }
            Self::validate_node_shape(previous, depth + 1)?;
        } else if node.receipt.predecessor.is_some() {
            return Err(invalid("missing_predecessor_receipt"));
        }
        Ok(())
    }

    fn validate_chain(
        plan: &Plan,
        node: &AdmissionNode,
        expected_phase: QualificationPhase,
        configuration_digest: &str,
        depth: usize,
    ) -> Result<(), ContractError> {
        if depth >= MAX_QUALIFICATION_RECEIPT_DEPTH
            || node.receipt.phase != expected_phase
            || node.receipt.campaign
                != plan
                    .qualification
                    .as_ref()
                    .map_or("", |q| q.campaign.as_str())
            || node.receipt.configuration_digest != configuration_digest
        {
            return Err(invalid("phase_campaign_or_configuration_mismatch"));
        }
        validate_node_metadata(plan, node)?;
        validate_lanes(plan, node, node.previous.as_deref())?;
        match (expected_phase.predecessor(), node.previous.as_deref()) {
            (None, None) => Ok(()),
            (Some(previous_phase), Some(previous)) => {
                if !run_ref_before(previous.receipt.run, node.receipt.run)
                    || previous.receipt.source_sha != node.receipt.source_sha
                {
                    return Err(invalid("unordered_or_source_changed_in_lineage"));
                }
                Self::validate_chain(
                    plan,
                    previous,
                    previous_phase,
                    configuration_digest,
                    depth + 1,
                )
            }
            _ => Err(invalid("phase_predecessor_shape_mismatch")),
        }
    }
}
