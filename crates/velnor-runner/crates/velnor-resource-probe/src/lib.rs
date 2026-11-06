// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod capacity;
mod cgroup;
#[cfg(target_os = "linux")]
pub mod cleanup_observation;
#[cfg(target_os = "linux")]
pub mod cleanup_observer;

pub mod cleanup_observer_wire;
mod error;
#[cfg(target_os = "linux")]
pub mod execution_channel;

pub mod execution_channel_wire;
pub mod execution_publication;
#[cfg(unix)]
#[path = "execution_channel/request.rs"]
pub mod execution_request;
mod filesystem;
pub mod job_facts;
pub mod job_facts_wire;
pub mod job_hook;
mod load;
mod memory;
mod mountinfo;
mod pressure;
mod record;
mod sample;

pub mod storage_init_contract;
mod units;
pub mod worker_binding;

pub use cgroup::{CpuHierarchy, observe_cpu_hierarchy_in};
pub use error::ProbeError;
pub use filesystem::InodePolicy;
pub use record::{CpuAncestorLimit, MAX_OUTPUT_BYTES, ProbeRecord};
pub use sample::sample;
