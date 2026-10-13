//! Detector registry contract: entry shape and schema.
//!
//! The [`DetectorEntry`] type plus [`DETECTION_SCHEMA`] live here; the
//! detector array itself stays in the composition root (it holds adapter
//! function pointers, which would invert the dependency DAG from here).
//! [`Stack`](velnor_actions_contract::stack::Stack) is the closed
//! per-stack dispatch enum: orchestrator dispatch matches on it, never
//! on stack-id spellings.

use crate::discover::FileIndex;
use crate::propose::StackCandidate;

/// Detector registry entry: stack id, record schema, implementation.
pub type DetectorEntry = (&'static str, u32, fn(&FileIndex) -> Vec<StackCandidate>);

/// Detector record schema: `{ stack_id, project_root, manifest }`.
pub const DETECTION_SCHEMA: u32 = 1;
