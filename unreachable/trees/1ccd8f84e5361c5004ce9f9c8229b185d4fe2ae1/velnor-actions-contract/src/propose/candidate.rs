//! Neutral task proposals: detector candidates plus analysis outcomes.
//!
//! [`StackCandidate`] is detector output (stack plus unit root); the
//! orchestrator dispatches per stack for record construction and
//! inventory. [`CandidateOutcome`] reports one unit-analysis result;
//! malformed units fail even when their stack is ignored.

use crate::discover::{DetectError, DetectionStatus};

/// One detector sighting: stack plus unit root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackCandidate {
    /// Registered stack id.
    pub stack_id: String,
    /// Repository-relative unit directory; empty for the repository root.
    pub unit_root: String,
}

/// Outcome of the orchestrator's unit-analysis request for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateOutcome {
    /// Candidate evidence path this outcome belongs to.
    pub manifest: String,
    /// Whether unit analysis succeeded.
    pub metadata_ok: bool,
    /// Adapter diagnostic when `metadata_ok` is false.
    pub diagnostic: Option<String>,
}

/// Fail on malformed units even when their stack is ignored.
///
/// # Errors
///
/// Returns [`DetectError::MalformedUnit`] for the first outcome whose
/// analysis request failed, regardless of selection state.
pub fn check_candidate_outcomes(
    statuses: Vec<DetectionStatus>,
    outcomes: &[CandidateOutcome],
) -> Result<Vec<DetectionStatus>, DetectError> {
    for outcome in outcomes {
        if !outcome.metadata_ok {
            return Err(DetectError::MalformedUnit {
                unit: outcome.manifest.clone(),
                diagnostic: outcome
                    .diagnostic
                    .clone()
                    .unwrap_or_else(|| "metadata_failed".to_owned()),
            });
        }
    }
    Ok(statuses)
}
