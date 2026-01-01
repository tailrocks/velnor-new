//! Plan rendering: deterministic text, stacks, and critical path.
//!
//! [`plan`] renders the human-readable plan report from a validated
//! preparation ([`plan_stacks`] covers the per-stack sections);
//! [`critical_path`] derives the longest obligation chain;
//! [`plan_output_limits`] enforces the fail-closed matrix and job-output
//! budgets the plan job promotes downstream.

pub mod critical_path;
pub mod plan;
pub mod plan_output_limits;
pub mod plan_stacks;
