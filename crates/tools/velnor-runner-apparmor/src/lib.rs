//! Linux runner `AppArmor` admission over the host journal error.
//!
//! Extracted from velnor-runner-host; the only dependency is the
//! `HostError` every admission call returns.

mod apparmor;

#[cfg(any(test, feature = "test-support"))]
pub use apparmor::test_runner_profile_admission;
pub use apparmor::{RunnerProfileAdmission, verify_runner_profile};
