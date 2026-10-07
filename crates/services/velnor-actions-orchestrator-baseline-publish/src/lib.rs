//! Event-time trusted-evidence publication for protected pushes.
//!
//! Extracted from the orchestrator hub: [`baseline_publish`] binds the
//! downloaded plan to the protected-push refs, builds the exact staged
//! manifest later lookups consume, and derives the upload artifact
//! name, with no hub dependency — the run key resolves through core,
//! schema and manifest types arrive via the merge-ports contract and
//! the merge crate, and the plan bound travels with the retrieve
//! crate. The hub keeps no publish items: its request dispatcher
//! and plan-file stager call into this crate directly.

pub mod baseline_publish;
