//! Check-to-plan binding for qualified named-check execution.
//!
//! [`binding`] re-derives the planner's lanes for a discovered check
//! and refuses any definition, task, input, or lane drift before the
//! check runs. The execute phase calls this gate, then runs the bound
//! check and persists its evidence.

pub mod binding;
