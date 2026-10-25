mod candidate;
mod identity;
mod task;

pub use candidate::{CandidateOutcome, StackCandidate, check_candidate_outcomes};
pub use identity::{IdentityInputs, component_id_for_unit, project_root_for_unit_path};
pub use task::ProposedTask;
