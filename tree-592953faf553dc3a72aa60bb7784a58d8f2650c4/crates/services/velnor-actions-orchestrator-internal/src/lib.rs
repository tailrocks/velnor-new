//! Event-time internal JSON operations: plan entrypoint, request files, responses.
//!
//! Extracted from the orchestrator hub as one ownership boundary: the
//! `internal` plan-v1 entrypoint plus `internal_request` file
//! materialization and response splitting form a re-export cycle, so
//! they move together with the two port-implementing riders they need
//! (`cover_baseline` behind [`CoverBaselinePort`], `merge_request`
//! behind [`RequestPort`]). Both ports already live in their seam
//! crates; the hub keeps every entrypoint byte-identical through its
//! `api` re-export.
//!
//! [`CoverBaselinePort`]: velnor_actions_orchestrator_merge_ports::CoverBaselinePort
//! [`RequestPort`]: velnor_actions_orchestrator_merge_request_ports::RequestPort

pub mod cover_baseline;
pub mod internal;
pub mod internal_request;
pub mod merge_entry;
pub mod merge_request;
