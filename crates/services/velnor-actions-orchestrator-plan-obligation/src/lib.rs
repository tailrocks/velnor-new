//! Per-obligation plan identities and changed-work dispositions.
//!
//! Extracted from the orchestrator hub: [`plan_obligation`] attaches
//! content identities to every universe member, then assigns exactly
//! one disposition — changed obligations execute unconditionally
//! while unchanged ones take the reuse outcome for later baseline
//! classification. The module had zero hub imports, so it moves as
//! a dependency-free leaf; the hub plan caller imports it back.

pub mod plan_obligation;
