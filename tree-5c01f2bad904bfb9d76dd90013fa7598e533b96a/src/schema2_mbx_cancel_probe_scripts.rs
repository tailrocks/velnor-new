//! Bounded shell scripts for the isolated hosted cancellation jobs.

#[path = "schema2_mbx_cancel_probe_controller_dispatch.rs"]
mod controller_dispatch;
#[path = "schema2_mbx_cancel_probe_controller_live.rs"]
mod controller_live;
#[path = "schema2_mbx_cancel_probe_controller_receipt.rs"]
mod controller_receipt;
#[path = "schema2_mbx_cancel_probe_observer.rs"]
mod observer;
#[path = "schema2_mbx_cancel_probe_victim.rs"]
mod victim;

pub(super) use controller_dispatch::{DISPATCH, GENERATE_ID};
pub(super) use controller_live::{cancel_exact, wait_readiness, wait_terminal};
pub(super) use controller_receipt::{CONTROLLER_RECEIPT, VALIDATE_CONTROLLER_RECEIPT};
pub(super) use observer::{
    OBSERVER_CACHE_BEFORE, OBSERVER_CLASSIFY, OBSERVER_EVIDENCE, OBSERVER_IMPORT_MEASURE,
    OBSERVER_REUSE_MEASURE,
};
pub(super) use victim::{
    BUILD_WORKSPACE, FETCH_SOURCE, PRE_SAVE_GUARD, PRE_SAVE_WAIT, VICTIM_IDENTITY,
    WRITE_VICTIM_RECEIPT,
};
