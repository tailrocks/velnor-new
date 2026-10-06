//! Source-only RootLinux candidate supplier, distinct from installed SDK authority.

use serde_json::Value;
use velnor_actions_mise::catalog::root_rust_candidate::RootRustCompilerCandidate;

use crate::OrchestratorError;

/// Private source image. No installed compiler, Foundation or SDK grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootRustCandidateRecipe {
    owner: RootRustCompilerCandidate,
}

impl RootRustCandidateRecipe {
    /// Resolve the closed candidate source without borrowing production SDK rights.
    pub(crate) fn root_linux(version: &str) -> Result<Self, OrchestratorError> {
        if version != env!("CARGO_PKG_VERSION") {
            return Err(invalid("generator_version"));
        }
        let owner = RootRustCompilerCandidate::root_linux(version)
            .map_err(|error| invalid(&error.to_string()))?;
        owner
            .verify_fresh()
            .map_err(|error| invalid(&error.to_string()))?;
        Ok(Self { owner })
    }

    /// Exact owner projection. Runtime admission remains separately sealed.
    pub(crate) fn projection(&self) -> Value {
        self.owner.projection()
    }

    /// Reconstruct all source and native-input bindings.
    pub(crate) fn verify_fresh(&self) -> Result<(), OrchestratorError> {
        self.owner
            .verify_fresh()
            .map_err(|error| invalid(&error.to_string()))
    }
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("root_rust_candidate_supplier_{problem}"),
    }
}
