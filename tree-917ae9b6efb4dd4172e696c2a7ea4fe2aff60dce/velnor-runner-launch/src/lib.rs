//! Job acquisition and worker launch (split from velnor-runner-host).
//!
//! Owns the launch state machine, admission, capacity, and the blocking
//! listen loop; host effects (journal, Docker, IPC) stay in the parent.

pub mod launch;
mod launch_blocking;

pub use launch::{LaunchReport, launch_once};
pub use launch_blocking::{ListenFault, launch_blocking};
