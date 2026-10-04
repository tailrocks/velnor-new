//! Data-only MBX publication authority, distinct from executable and source owners.
use super::{mbx_export_descriptor::MbxExportDescriptor, step::StepId};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Pure publication of independently admitted native exports.
/// Deserialization validates shape only; compiled recipe admission grants authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PureMbxProducer {
    /// Exact downstream native producer and compatibility cohort.
    pub descriptor: MbxExportDescriptor,
    /// Selected work; no arbitrary job predicate is accepted.
    pub selection: super::tool_producer_selection::ToolProducerSelection,
    /// Fresh qualified MBX executable preparation outside payload roots.
    pub installation_step: StepId,
    /// Authenticated same-run artifact and every retained historical origin.
    pub admission_step: StepId,
    /// Supported owner data-only physical/semantic bundle verification.
    pub verification_step: StepId,
    /// Fixed native bundle cache transport.
    pub save_step: StepId,
    /// Exact saved-key availability lookup; not a validation obligation.
    pub publication_step: StepId,
    /// Honest advisory availability and performance qualification evidence.
    pub report_step: StepId,
}

impl PureMbxProducer {
    /// Validate disjoint authority and canonical evidence identifiers.
    /// # Errors
    /// Rejects foreign selection, duplicate bindings and missing validation work.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.descriptor.validate()?;
        self.selection.validate()?;
        let tasks = self
            .descriptor
            .task_digests
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        if self.selection.tasks != tasks
            || self.selection.unconditional
            || (self.selection.cargo_fallback
                && self.descriptor.domain != super::mbx_export_descriptor::MbxCacheDomain::Helper)
            || (tasks.is_empty() && !self.selection.cargo_fallback)
        {
            return Err(invalid("foreign_task_selection"));
        }
        let ids = [
            &self.installation_step,
            &self.admission_step,
            &self.verification_step,
            &self.save_step,
            &self.publication_step,
            &self.report_step,
        ];
        for (index, id) in ids.iter().enumerate() {
            id.validate()?;
            if ids[..index].contains(id) {
                return Err(invalid("duplicate_evidence_binding"));
            }
        }
        Ok(())
    }

    /// Pure writer waits for its exact producer; covered jobs never allocate it.
    #[must_use]
    pub fn condition(&self) -> String {
        format!(
            "{} && needs.{}.result == 'success'",
            self.selection
                .condition(super::tool_producer::ToolCacheDomain::Full),
            self.descriptor.producer_job_id,
        )
    }

    /// Only Plan and the exact downstream export owner can precede this writer.
    #[must_use]
    pub fn needs(&self) -> Vec<String> {
        let mut needs = vec!["plan".to_owned(), self.descriptor.producer_job_id.clone()];
        needs.sort();
        needs.dedup();
        needs
    }

    /// Save only authenticated useful state verified by the native data owner.
    #[must_use]
    pub fn save_condition(&self) -> String {
        format!(
            "{} && steps.{}.outputs.admitted == 'true' && steps.{}.outputs.verified == 'true' && steps.{}.outputs.useful == 'true'",
            super::cache_trust::CACHE_SAVE_CONDITION,
            self.admission_step.as_str(),
            self.verification_step.as_str(),
            self.admission_step.as_str(),
        )
    }

    /// Immutable semantic snapshot suffix; unchanged native state produces no save.
    /// # Errors
    pub fn save_key(&self) -> Result<String, ContractError> {
        self.validate()?;
        Ok(format!(
            "{}snapshot-${{{{ steps.{}.outputs.semantic_digest }}}}",
            self.descriptor.cache_prefix()?,
            self.verification_step.as_str()
        ))
    }

    /// Optional transport failures remain explicit and independent from task success.
    #[must_use]
    pub fn publication_condition(&self) -> String {
        format!(
            "always() && {} && steps.{}.outputs.admitted == 'true' && steps.{}.outputs.verified == 'true'",
            super::cache_trust::CACHE_TRUSTED_PUSH_EXPR,
            self.admission_step.as_str(),
            self.verification_step.as_str(),
        )
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("mbx_producer", reason)
}
