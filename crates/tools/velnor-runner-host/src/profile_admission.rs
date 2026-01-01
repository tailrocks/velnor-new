//! Host facade for the sealed Linux `AppArmor` admission token.

pub use velnor_runner_apparmor::RunnerProfileAdmission;

use crate::HostError;

/// Verify the installed enforcing policy against the identity approved by this binary.
///
/// The token has no public constructor. The current binary intentionally has
/// no approved policy digest, so verification remains fail-closed until a
/// reviewed policy is added to the build.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the approved policy is unavailable or
/// any installed profile is missing, non-enforcing, or has a different hash.
pub fn verify_runner_profile_admission() -> Result<RunnerProfileAdmission, HostError> {
    velnor_runner_apparmor::verify_runner_profile()
}
