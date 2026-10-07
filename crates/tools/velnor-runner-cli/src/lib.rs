//! Clap parsing and presentation for `velnor-host`.

mod args;
mod compare;
mod daemon_run;
mod dispatch;
mod linux_admission;
mod service;

pub use args::Cli;
pub use dispatch::run;
pub use linux_admission::{LinuxAdmissionError, with_verified_pool_policy};
