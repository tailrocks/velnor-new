//! Merge-request seam contract: the request port.
//!
//! This crate sits at the bottom of the orchestrator family: it owns the
//! [`RequestPort`] trait through which merge-request assembly calls the
//! run-key behavior owned by the core crate. Neither side depends on
//! the other; the hub implements the port.

mod request_port;

pub use request_port::RequestPort;
