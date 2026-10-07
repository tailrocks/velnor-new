//! Merge-request seam contract: the request port.
//!
//! This crate sits at the bottom of the orchestrator family: it owns the
//! [`RequestPort`] trait through which merge-request assembly calls the
//! run-key and staged-report behavior that stays in the hub
//! (`internal_request`, `retrieve_reports`). Neither side depends on the
//! other; the hub implements the port.

mod request_port;

pub use request_port::RequestPort;
