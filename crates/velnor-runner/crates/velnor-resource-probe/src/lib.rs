//! Fixed, bounded Linux guest resource measurements for the controller-only probe image.

mod capacity;
mod cgroup;
#[cfg(target_os = "linux")]
pub mod cleanup_observation;
#[cfg(target_os = "linux")]
pub mod cleanup_observer;
/// Cleanup request and record types shared by the host and guest.
pub mod cleanup_observer_wire;
mod error;
#[cfg(target_os = "linux")]
pub mod execution_channel;
/// Data records that the host and Linux guest processes use.
pub mod execution_channel_wire;
pub mod execution_publication;
mod filesystem;
pub mod job_facts;
/// Job facts records shared by the host and Linux guest.
pub mod job_facts_wire;
mod load;
mod memory;
mod mountinfo;
mod pressure;
mod record;
mod sample;
/// Storage initialization mode requirements shared by the host and guest.
pub mod storage_init_contract;
mod units;
pub mod worker_binding;

pub use error::ProbeError;
pub use filesystem::InodePolicy;
pub use record::{CpuAncestorLimit, MAX_OUTPUT_BYTES, ProbeRecord};
pub use sample::sample;

#[cfg(test)]
mod tests;
