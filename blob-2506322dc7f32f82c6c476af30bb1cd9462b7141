//! Shared validation for serialized plan responses.

use velnor_actions_contract::canonical_json_bytes;

use crate::OrchestratorError;
use crate::internal::{PlanResponse, check_schema, internal, internal_contract};

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
