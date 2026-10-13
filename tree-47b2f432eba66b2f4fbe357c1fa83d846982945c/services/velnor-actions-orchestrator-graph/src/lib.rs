//! Plan-graph planning: obligations, closures, identities, lanes.
//!
//! Fifth layer of the orchestrator family: shapes discovery output and
//! selection into the obligation graph (identities, input closures,
//! named-check lanes). Takes caller-computed argv so the graph never
//! routes tools; the runner executes the shaped plan.

pub mod internal_plan;
