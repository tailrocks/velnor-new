//! Typed verification scope for generated workflow plans.

use super::baseline::{BaselineStatus, PlanBaseline};
use super::plan::{ObligationDecision, PlanObligation, WorkflowEvent};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Verification universe recorded by a plan.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationScope {
    /// Verify the affected obligation set and use eligible evidence for the rest.
    #[default]
    Affected,
    /// Verify every obligation directly in this run.
    Full,
}

impl VerificationScope {
    /// Validate scope constraints against the triggering event and obligations.
    ///
    /// # Errors
    ///
    /// Returns an error when a scheduled plan is not full or a full plan carries
    /// reuse or baseline evidence.
    pub(super) fn validate(
        self,
        event: WorkflowEvent,
        obligations: &[PlanObligation],
    ) -> Result<(), ContractError> {
        if event == WorkflowEvent::Schedule && self != Self::Full {
            return Err(ContractError::identity("scope", "schedule_requires_full"));
        }
        if self == Self::Full {
            for obligation in obligations {
                if obligation.decision != ObligationDecision::Execute {
                    return Err(ContractError::identity("scope", "full_requires_execute"));
                }
                if obligation.baseline_proof.is_some() {
                    return Err(ContractError::identity(
                        "scope",
                        "full_forbids_baseline_proof",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validate baseline metadata recorded for this verification scope.
    ///
    /// # Errors
    ///
    /// Returns an error when a full plan carries a used baseline or a reason
    /// other than `full_verification`.
    pub(super) fn validate_baseline(self, baseline: &PlanBaseline) -> Result<(), ContractError> {
        if self != Self::Full {
            return Ok(());
        }
        if baseline.status() != BaselineStatus::Unavailable {
            return Err(ContractError::identity(
                "baseline",
                "full_requires_unavailable",
            ));
        }
        if baseline.reason() != Some("full_verification") {
            return Err(ContractError::identity(
                "baseline",
                "full_requires_full_verification",
            ));
        }
        Ok(())
    }

    /// Validate the optional changed-work base for this scope.
    ///
    /// # Errors
    ///
    /// Returns an error when a full plan carries a base commit.
    pub(super) fn validate_base(self, base: Option<&str>) -> Result<(), ContractError> {
        if self == Self::Full && base.is_some() {
            return Err(ContractError::identity("base", "full_requires_no_base"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::digest_b3;
    use crate::config::RunnerSelection;
    use crate::ids::{artifact_id_for_plan, plan_id_for_run, run_key_for_ci};
    use crate::workflow::baseline::{BaselineProof, PlanBaseline};
    use crate::workflow::plan::{Plan, PlanGenerator, PlanMatrix, PlanRunner};
    use crate::workflow::trust::Trust;

    const TASK: &str = "stack/rust/demo/clippy/default";

    fn obligation(
        decision: ObligationDecision,
        baseline_proof: Option<BaselineProof>,
    ) -> PlanObligation {
        let digest = digest_b3(b"fixture");
        PlanObligation {
            task_id: "stack/rust/demo/clippy/default".to_owned(),
            job_id: "plan".to_owned(),
            decision,
            reason: "fixture".to_owned(),
            task_digest: digest.clone(),
            input_digest: digest.clone(),
            execution_identity: crate::TaskExecutionIdentity::new(
                &crate::digest_b3(b"fixture-graph"),
                &crate::digest_b3(b"fixture-toolchain"),
                &crate::digest_b3(b"fixture-mbx"),
                &crate::digest_b3(b"fixture-platform"),
                "default",
            )
            .expect("execution identity"),
            closure_digest: digest,
            baseline_proof,
        }
    }

    fn plan(
        scope: VerificationScope,
        event: WorkflowEvent,
        obligation: PlanObligation,
    ) -> Result<Plan, ContractError> {
        let run_key = run_key_for_ci(1, 1);
        Ok(Plan {
            producers: crate::workflow::ProducerInventory::default(),
            schema: 1,
            run_key: run_key.clone(),
            plan_id: plan_id_for_run(&run_key)?,
            base: None,
            head: "ab".repeat(20),
            event,
            scope,
            runner: PlanRunner {
                label: "ubuntu-26.04".to_owned(),
                selection: RunnerSelection::LatestDefault,
            },
            trust: Trust::Pr,
            baseline: PlanBaseline::unavailable(match scope {
                VerificationScope::Affected => None,
                VerificationScope::Full => Some("full_verification"),
            })?,
            generator: PlanGenerator {
                version: "0.1.0".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
                sha256: "ab".repeat(32),
            },
            packages: Vec::new(),
            obligations: vec![obligation],
            matrix: PlanMatrix {
                include: Vec::new(),
            },
            task_ids: vec![TASK.to_owned()],
            warnings: Vec::new(),
            edges: Vec::new(),
        })
    }

    #[test]
    fn default_and_wire_names_are_affected_and_full() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(VerificationScope::default(), VerificationScope::Affected);
        assert_eq!(
            serde_json::to_string(&VerificationScope::Affected)?,
            "\"affected\""
        );
        assert_eq!(serde_json::to_string(&VerificationScope::Full)?, "\"full\"");
        Ok(())
    }

    #[test]
    fn omitted_plan_scope_defaults_to_affected() -> Result<(), Box<dyn std::error::Error>> {
        let plan = plan(
            VerificationScope::Affected,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        let mut value = serde_json::to_value(plan)?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| std::io::Error::other("serialized plan is not an object"))?;
        object.remove("scope");
        let decoded: Plan = serde_json::from_value(value)?;
        assert_eq!(decoded.scope, VerificationScope::Affected);
        Ok(())
    }

    #[test]
    fn scheduled_affected_scope_is_rejected() {
        let result = VerificationScope::Affected.validate(WorkflowEvent::Schedule, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn full_scope_rejects_an_used_baseline() -> Result<(), ContractError> {
        let run_key = run_key_for_ci(1, 1);
        let mut plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.baseline = PlanBaseline::used(
            &"ab".repeat(20),
            99,
            4242,
            &artifact_id_for_plan(&run_key)?,
            &digest_b3(b"manifest"),
        )?;
        assert!(plan.validate().is_err());
        Ok(())
    }

    #[test]
    fn full_scope_rejects_an_unavailable_no_work_baseline() -> Result<(), ContractError> {
        let mut plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.baseline = PlanBaseline::unavailable(Some("no_work"))?;
        assert!(plan.validate().is_err());
        Ok(())
    }

    #[test]
    fn full_scope_rejects_an_unavailable_wrong_reason() -> Result<(), ContractError> {
        let mut plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.baseline = PlanBaseline::unavailable(Some("baseline_lookup_deferred"))?;
        assert!(plan.validate().is_err());
        Ok(())
    }

    #[test]
    fn full_scope_requires_a_cleared_base() -> Result<(), ContractError> {
        let mut plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.base = Some("ab".repeat(20));
        assert!(plan.validate().is_err());
        Ok(())
    }

    #[test]
    fn forged_full_scope_cannot_cover_an_obligation() -> Result<(), ContractError> {
        let run_key = run_key_for_ci(1, 1);
        let proof = BaselineProof::new(
            &"ab".repeat(20),
            99,
            4242,
            &artifact_id_for_plan(&run_key)?,
            &digest_b3(b"manifest"),
        )?;
        let forged = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::CoveredByTrustedBaseline, Some(proof)),
        )?;
        assert!(forged.validate().is_err());
        Ok(())
    }

    #[test]
    fn full_scope_cannot_reuse_a_task_result() -> Result<(), ContractError> {
        let plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::ReusedFromTaskCache, None),
        )?;
        assert!(plan.validate().is_err());
        Ok(())
    }

    #[test]
    fn full_execute_plan_validates() -> Result<(), ContractError> {
        let plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Push,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.validate()
    }

    #[test]
    fn scheduled_full_execute_plan_validates() -> Result<(), ContractError> {
        let plan = plan(
            VerificationScope::Full,
            WorkflowEvent::Schedule,
            obligation(ObligationDecision::Execute, None),
        )?;
        plan.validate()
    }

    #[test]
    fn full_scope_rejects_baseline_proof_even_for_execute() -> Result<(), ContractError> {
        let run_key = run_key_for_ci(1, 1);
        let proof = BaselineProof::new(
            &"ab".repeat(20),
            99,
            4242,
            &artifact_id_for_plan(&run_key)?,
            &digest_b3(b"manifest"),
        )?;
        let result = VerificationScope::Full.validate(
            WorkflowEvent::Push,
            &[obligation(ObligationDecision::Execute, Some(proof))],
        );
        assert!(result.is_err());
        Ok(())
    }
}
