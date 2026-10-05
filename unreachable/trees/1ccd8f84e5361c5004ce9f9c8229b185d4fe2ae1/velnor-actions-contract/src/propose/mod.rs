//! Stack-neutral task proposals: candidates, tasks, identity inputs.
//!
//! Adapters propose typed obligations here; the orchestrator computes
//! digests and schedules the planned graph. See each module for the
//! §4-step-4 reuse rationale.

mod candidate;
mod identity;
mod task;

pub use candidate::{CandidateOutcome, StackCandidate, check_candidate_outcomes};
pub use identity::{IdentityInputs, component_id_for_unit, project_root_for_unit_path};
pub use task::ProposedTask;
