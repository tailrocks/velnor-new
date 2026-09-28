//! Generation coordination: root, config, discovery, IR, and writes.
//!
//! Composes the contract, Rust, Mise, actionlint, and workflow-renderer
//! adapters. This crate launches no child invocations, builds no fixed
//! vectors itself, and assembles no workflow text: execution belongs to
//! Mise, vectors to [`vectors`] via Mise requests, text to the renderer.

mod config;
mod discover;
mod error;
mod evidence;
mod generate;
mod init;
mod internal;
mod merge;
mod plan;
mod prepare;
mod root;
mod select;
mod vectors;
mod workflow;

pub use discover::{Discovery, PlannedWorkspace};
pub use error::OrchestratorError;
pub use generate::{GenerateOptions, GenerateReport, generate};
pub use init::{InitReport, init_config};
pub use internal::plan_internal;
pub use merge::merge_internal;
pub use plan::plan_text;
pub use prepare::{GenerationPreparation, prepare};
pub use root::resolve_root;
pub use workflow::{CHECKOUT_USES, DEFAULT_RUNNER_LABEL, WorkflowPlan};

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;
