//! Execution-input provisioning: sources, caches, routes, steps, env.
//!
//! Fourth layer of the orchestrator family: prepares everything a step
//! needs to run (vendored sources, caches, tool vectors and routing,
//! lock audit, tofu cache, matrix-step construction and step env).
//! Builds on discovery; the runner consumes provisioned inputs.

pub mod lock_audit;
pub mod matrix_step;
pub mod source_cache;
pub mod source_prep;
pub mod tofu_cache;
pub mod vectors;
