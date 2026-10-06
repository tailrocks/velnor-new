//! Exact pure-producer admissions and runner-owned scheduling context.
use super::{
    ObligationDecision, Plan, PureMbxProducer, PureToolProducer, SourceProducer, ToolCacheDomain,
    WorkflowEvent,
};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Independently observed facts used by the fixed producer predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerEventContext {
    /// Literal triggering Git ref.
    pub reference: String,
    /// Repository default branch.
    pub default_branch: String,
    /// Runner-observed branch protection.
    pub protected: bool,
    /// Actual Plan Cargo fallback, never inferred from a task name.
    pub cargo_fallback: bool,
}

/// Required folding policy independent of cache qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerPolicy {
    /// Consumers retain their declared cold preparation path.
    AdvisoryFallback,
    /// Exact executable availability is a declared required precondition.
    Mandatory,
}

/// Closed producer role; IDs never confer producer authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProducerRole {
    /// Native source production with immutable source identity.
    Source {
        /// Exact compiled source role, immutable identity and evidence bindings.
        producer: SourceProducer,
    },
    /// Executable production with exact qualified tool identity.
    Tool {
        /// Exact qualified executable payload and terminal evidence bindings.
        producer: PureToolProducer,
    },
    /// Publication of authenticated native MBX data, independent from task obligations.
    Mbx {
        /// Exact native export cohort and terminal evidence bindings.
        producer: PureMbxProducer,
    },
}

/// One job admitted from validated workflow IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerAdmission {
    /// Literal job identity in the Required needs inventory.
    pub job_id: String,
    /// Complete compiled producer authority.
    pub role: ProducerRole,
    /// Exact terminal policy.
    pub policy: ProducerPolicy,
}

/// Plan-bound producer inventory, separate from actual verification tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerInventory {
    /// Runner context, absent for local planning.
    pub context: Option<ProducerEventContext>,
    /// Canonical producer admissions sorted by job ID.
    pub entries: Vec<ProducerAdmission>,
}

impl ProducerAdmission {
    pub(super) fn validate_role(&self) -> Result<(), ContractError> {
        match &self.role {
            ProducerRole::Source { producer } => {
                producer.validate()?;
                if self.policy != ProducerPolicy::AdvisoryFallback {
                    return Err(ContractError::identity(
                        "producers",
                        "mandatory_source_forbidden",
                    ));
                }
            }
            ProducerRole::Tool { producer } => producer.validate()?,
            ProducerRole::Mbx { producer } => {
                producer.validate()?;
                if self.policy != ProducerPolicy::AdvisoryFallback {
                    return Err(ContractError::identity(
                        "producers",
                        "mandatory_mbx_forbidden",
                    ));
                }
            }
        }
        Ok(())
    }
    /// Canonical producer job identity, recomputed from the exact typed role.
    /// # Errors
    /// Rejects a descriptor that cannot be canonically encoded.
    pub fn expected_job_id(&self) -> Result<String, ContractError> {
        match &self.role {
            ProducerRole::Source { producer } => {
                let prefix = match producer.role {
                    super::SourceProducerRole::Cargo => "rust-source",
                    super::SourceProducerRole::Npm => "npm-source",
                    super::SourceProducerRole::Bun => "bun-source",
                    super::SourceProducerRole::Gradle => "gradle-source",
                    super::SourceProducerRole::Tofu => "tofu-provider-source",
                };
                Ok(format!(
                    "{prefix}-{}",
                    crate::digest_b3(producer.source_identity.as_bytes())
                ))
            }
            ProducerRole::Tool { producer } => {
                let canonical = crate::canonical_json_str(&producer.descriptor)?;
                Ok(format!(
                    "tools-{}-{}",
                    producer.descriptor.domain.name(),
                    crate::digest_b3(canonical.as_bytes())
                ))
            }
            ProducerRole::Mbx { producer } => Ok(format!(
                "mbx-{}-{}",
                producer.descriptor.domain.name(),
                producer.descriptor.identity()?
            )),
        }
    }

    /// Recomputed terminal identity owned by this exact role.
    /// # Errors
    /// Rejects malformed MBX descriptors rather than accepting a wire identity claim.
    pub fn identity(&self) -> Result<String, ContractError> {
        match &self.role {
            ProducerRole::Source { producer } => Ok(producer.source_identity.clone()),
            ProducerRole::Tool { producer } => Ok(producer.descriptor.immutable_identity.clone()),
            ProducerRole::Mbx { producer } => producer.descriptor.identity(),
        }
    }

    /// Exact selected work served by this producer.
    #[must_use]
    pub fn selection(&self) -> &super::ToolProducerSelection {
        match &self.role {
            ProducerRole::Source { producer } => &producer.selection,
            ProducerRole::Tool { producer } => &producer.selection,
            ProducerRole::Mbx { producer } => &producer.selection,
        }
    }
}

impl ProducerInventory {
    /// Validate exact typed admissions, without granting task ownership.
    /// # Errors
    /// Rejects duplicate roles, foreign task selections and obligation owners.
    pub fn validate(&self, plan: &Plan) -> Result<(), ContractError> {
        if self
            .entries
            .windows(2)
            .any(|pair| pair[0].job_id >= pair[1].job_id)
        {
            return Err(ContractError::identity(
                "producers",
                "noncanonical_inventory",
            ));
        }
        for admission in &self.entries {
            super::validate_job_id(&admission.job_id)?;
            if admission.job_id != admission.expected_job_id()? {
                return Err(ContractError::identity(
                    "producers",
                    "job_identity_mismatch",
                ));
            }
            admission.validate_role()?;
            if plan
                .obligations
                .iter()
                .any(|obligation| obligation.job_id == admission.job_id)
                || admission
                    .selection()
                    .tasks
                    .iter()
                    .any(|task| plan.task_ids.binary_search(task).is_err())
            {
                return Err(ContractError::identity(
                    "producers",
                    "foreign_task_authority",
                ));
            }
        }
        Ok(())
    }

    /// Evaluate the same protected-default-push and selected-work predicate as IR.
    #[must_use]
    pub fn eligible(&self, admission: &ProducerAdmission, plan: &Plan) -> bool {
        let Some(context) = &self.context else {
            return false;
        };
        if plan.event != WorkflowEvent::Push
            || !context.protected
            || context.reference != format!("refs/heads/{}", context.default_branch)
            || context.default_branch.is_empty()
        {
            return false;
        }
        if matches!(&admission.role, ProducerRole::Tool { producer } if producer.descriptor.domain == ToolCacheDomain::Planning)
        {
            return true;
        }
        let selection = admission.selection();
        selection.unconditional
            || (selection.cargo_fallback && context.cargo_fallback)
            || plan.obligations.iter().any(|obligation| {
                obligation.decision != ObligationDecision::CoveredByTrustedBaseline
                    && selection.tasks.binary_search(&obligation.task_id).is_ok()
            })
    }
}
