//! Fixed, bounded Linux guest resource measurements for the controller-only probe image.

mod capacity;
mod cgroup;
#[cfg(target_os = "linux")]
pub mod cleanup_observation;
mod error;
#[cfg(target_os = "linux")]
pub mod execution_channel;
mod filesystem;
mod load;
mod memory;
mod mountinfo;
mod pressure;
mod record;
mod sample;
mod units;
pub mod worker_binding;

pub use error::ProbeError;
pub use filesystem::InodePolicy;
pub use record::{CpuAncestorLimit, MAX_OUTPUT_BYTES, ProbeRecord};
pub use sample::sample;

#[cfg(test)]
mod tests;
