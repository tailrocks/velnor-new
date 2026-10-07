//! Job acquisition and worker launch (split from velnor-runner-host).
//!
//! Owns the launch state machine, admission, capacity, and the blocking
//! listen loop; host effects (journal, Docker, IPC) stay in the parent.

pub mod discovery_intents;
pub mod launch;
mod launch_blocking;

/// GitHub pool-admission evidence accepted by the typed Linux launch path.
///
/// These are direct re-exports so callers share GitHub's opaque proof type;
/// this crate does not provide a constructor or alternate issuer.
pub mod linux {
    pub use velnor_runner_github::policy::{
        PolicyGap, PolicyMismatch, PoolAdmissionEvidence, VerifiedPoolPolicy,
    };
}

pub use launch::control::{
    ControlOpenError, DrainOutcome, DrainRequestOutcome, DrainUnknown, ResumeBlockReason,
    ResumeOutcome, request_drain_blocking, resume_blocking, wait_drained_blocking,
};
pub use launch::{LaunchReport, launch_once};
pub use launch_blocking::{ListenFault, launch_blocking};
