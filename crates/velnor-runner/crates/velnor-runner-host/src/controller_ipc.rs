//! Private local requests handled by the process that owns the journal lineage.

mod client;
mod offline;
mod server;
mod wire;

pub use client::controller_readiness_status;
pub use client::{ConnectionCensus, wait_for_connection_census};
pub use client::{
    ConnectionSetupReceipt, RunnerGroupReceipt, SelectedSetReceipt,
    wait_for_connection_setup_receipt,
};
pub use client::{SessionObservation, SessionObservationStatistics, wait_for_session_observation};
pub use client::{
    reactivate_connection_if_drained, run_doctor_probe, wait_for_connection_detached,
    wait_for_connection_drain, wait_for_drain,
};
pub(crate) use server::{
    ControlServer, ControlState, ControllerReadinessProof, SessionPollReceipt, SetupReceiptInput,
};
