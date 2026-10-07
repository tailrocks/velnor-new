//! Typed gate between pool verification and a Linux launch adapter.
//!
//! A pool proof alone does not prove an individual job offer or request-to-JIT
//! routing. Callers must keep those checks and the durable capacity fence in
//! their launch adapter.

use std::error::Error;
use std::fmt;

use velnor_runner_launch::linux::{
    PolicyGap, PolicyMismatch, PoolAdmissionEvidence, VerifiedPoolPolicy,
};

/// Why the Linux launch adapter was not called or why it failed.
#[derive(Debug, PartialEq, Eq)]
pub enum LinuxAdmissionError<E> {
    /// Pool evidence was incomplete, stale, or could not prove effective routing.
    Unknown(PolicyGap),
    /// Observed pool policy contradicts configured identity or restrictions.
    Rejected(PolicyMismatch),
    /// The launch adapter returned an error after receiving the verified pool token.
    Launch(E),
}

impl<E: fmt::Display> fmt::Display for LinuxAdmissionError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(gap) => write!(f, "pool policy is unproven: {gap:?}"),
            Self::Rejected(mismatch) => write!(f, "pool policy rejected: {mismatch:?}"),
            Self::Launch(error) => write!(f, "Linux launch failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for LinuxAdmissionError<E> {}

/// Call a Linux launch adapter only when given the verifier's opaque pool proof.
///
/// This is a pool-level gate only. The callback remains responsible for
/// per-offer trust, profile admission, and durable state admission before any
/// runner/session effects. `Unknown` and `Rejected` never invoke the callback.
///
/// # Errors
///
/// Returns `Unknown` or `Rejected` without invoking `launch` when the verifier
/// does not provide an opaque verified pool token. Returns `Launch` when the
/// callback itself fails.
pub fn with_verified_pool_policy<T, E>(
    evidence: PoolAdmissionEvidence,
    launch: impl FnOnce(VerifiedPoolPolicy) -> Result<T, E>,
) -> Result<T, LinuxAdmissionError<E>> {
    match evidence {
        PoolAdmissionEvidence::Verified(policy) => {
            launch(*policy).map_err(LinuxAdmissionError::Launch)
        }
        PoolAdmissionEvidence::Unknown(gap) => Err(LinuxAdmissionError::Unknown(gap)),
        PoolAdmissionEvidence::Rejected(mismatch) => Err(LinuxAdmissionError::Rejected(mismatch)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{LinuxAdmissionError, with_verified_pool_policy};
    use velnor_runner_launch::linux::{PolicyGap, PolicyMismatch, PoolAdmissionEvidence};

    #[test]
    fn unknown_pool_evidence_never_calls_linux_launch_adapter() {
        let called = Cell::new(false);
        let result = with_verified_pool_policy::<(), ()>(
            PoolAdmissionEvidence::Unknown(PolicyGap::EffectiveRoutingApplicabilityUnproven),
            |_| {
                called.set(true);
                Ok(())
            },
        );

        assert_eq!(
            result,
            Err(LinuxAdmissionError::Unknown(
                PolicyGap::EffectiveRoutingApplicabilityUnproven
            ))
        );
        assert!(!called.get());
    }

    #[test]
    fn rejected_pool_evidence_never_calls_linux_launch_adapter() {
        let called = Cell::new(false);
        let result = with_verified_pool_policy::<(), ()>(
            PoolAdmissionEvidence::Rejected(PolicyMismatch::RepositoryNotPrivate),
            |_| {
                called.set(true);
                Ok(())
            },
        );

        assert_eq!(
            result,
            Err(LinuxAdmissionError::Rejected(
                PolicyMismatch::RepositoryNotPrivate
            ))
        );
        assert!(!called.get());
    }
}
