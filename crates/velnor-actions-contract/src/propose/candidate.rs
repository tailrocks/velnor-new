use crate::discover::{DetectError, DetectionStatus};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackCandidate {
    pub stack_id: String,

    pub unit_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateOutcome {
    pub manifest: String,

    pub metadata_ok: bool,

    pub diagnostic: Option<String>,
}

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
