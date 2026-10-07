//! Covered-task export: typed dispositions to generated skip gates.
//!
//! [`covered_tasks`] collects the trusted-baseline covered task IDs of
//! a plan, encodes them for the plan job's `covered_tasks` output, and
//! answers per-task coverage queries for fetch and skip-gate decisions.

pub mod covered_tasks;
