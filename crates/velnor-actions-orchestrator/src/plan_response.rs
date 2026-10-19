//! Shared validation for serialized plan responses.

use velnor_actions_contract::{Plan, PlanMatrix, canonical_json_bytes};

use crate::OrchestratorError;
use crate::internal::{
    MATRIX_BUDGET_BYTES, PlanResponse, SCHEMA, check_schema, internal, internal_contract,
};

impl PlanResponse {
    /// Parse and validate the complete plan response before any consumer reads it.
    /// # Errors
    pub(crate) fn parse(response_json: &str) -> Result<Self, OrchestratorError> {
        let response: Self =
            serde_json::from_str(response_json).map_err(|_| internal("malformed_response"))?;
        response.validate()?;
        Ok(response)
    }

    /// Validate schema, plan invariants, and the duplicated matrix authority.
    /// # Errors
    pub(crate) fn validate(&self) -> Result<(), OrchestratorError> {
        check_schema(self.schema)?;
        self.plan.validate().map_err(internal_contract)?;
        let response_matrix = canonical_json_bytes(&self.matrix).map_err(internal_contract)?;
        let plan_matrix = canonical_json_bytes(&self.plan.matrix).map_err(internal_contract)?;
        if response_matrix != plan_matrix {
            return Err(internal("response_matrix_mismatch"));
        }
        Ok(())
    }
}

/// Validate a serialized response before the CLI publishes its sibling file.
/// # Errors
pub fn validate_plan_response(response_json: &str) -> Result<(), OrchestratorError> {
    PlanResponse::parse(response_json).map(|_| ())
}

/// Assemble the `plan-v1` response, staging trusted bytes when covered.
///
/// The manifest rides along only behind covered obligations: a
/// rejected manifest must never reach the plan artifact.
pub(super) fn plan_response(plan: Plan) -> PlanResponse {
    PlanResponse {
        schema: SCHEMA,
        matrix: plan.matrix.clone(),
        plan,
    }
}

/// Reject a matrix whose canonical bytes exceed the budget.
pub(super) fn check_matrix_budget(matrix: &PlanMatrix) -> Result<(), OrchestratorError> {
    let bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    if bytes.len() > MATRIX_BUDGET_BYTES {
        return Err(internal(&format!(
            "matrix_budget_exceeded:{}:broaden selection or reduce matrix entries",
            bytes.len()
        )));
    }
    Ok(())
}
