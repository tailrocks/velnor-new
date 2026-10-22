//! Private local requests handled by the process that owns the journal lineage.

mod client;
mod offline;
mod server;
mod wire;

pub use client::controller_readiness_status;
pub use client::{
    reactivate_connection_if_drained, run_doctor_probe, wait_for_connection_detached,
    wait_for_connection_drain, wait_for_drain,
};
pub(crate) use server::{
    ControlServer, ControlState, ControllerReadinessProof, SessionPollReceipt,
};
