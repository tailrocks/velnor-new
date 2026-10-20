//! Create one private `DinD` and one official Runner container.

use futures_util::Stream;
use std::{pin::Pin, time::Instant};
use tokio::io::AsyncWriteExt;
use velnor_runner_github::EncodedJit;

use crate::error::HostError;

const PLATFORM: &str = "linux/amd64";
const RUNNER_PIDS: i64 = 1_024;
const DIND_PIDS: i64 = 2_048;
const LOG_MAX_SIZE: &str = "10m";
const LOG_MAX_FILES: &str = "3";

mod config;
mod production;
mod spec;

pub(super) use config::{create_body, fixed_mounts};
#[cfg(test)]
pub(super) use config::{limits_for, tmpfs_options};
pub(crate) use production::start_bootstrap_pair;
pub(in crate::launch::bootstrap) use production::{create_spec_sha256, verify_pre_jit_created};
pub(crate) use spec::PairCreate;
pub(in crate::launch::bootstrap) use spec::create_spec;

/// Keep the attach stream and start claim after a failed start attempt.
#[must_use]
pub(crate) struct PendingBootstrapPair {
    _claim: crate::journal::BootstrapStartClaim,
    _attached: bollard::container::AttachContainerResults,
}

/// Keep start uncertainty and any attached Runner stream.
#[must_use]
pub(crate) struct PairStartFailure {
    error: HostError,
    pending: Option<Box<PendingBootstrapPair>>,
}

impl PairStartFailure {
    pub(crate) fn into_parts(self) -> (HostError, Option<PendingBootstrapPair>) {
        (self.error, self.pending.map(|pending| *pending))
    }

    pub(super) fn before_attach(error: HostError) -> Self {
        Self {
            error,
            pending: None,
        }
    }

    pub(super) fn after_attach(
        error: HostError,
        claim: crate::journal::BootstrapStartClaim,
        attached: bollard::container::AttachContainerResults,
    ) -> Self {
        Self {
            error,
            pending: Some(Box::new(PendingBootstrapPair {
                _claim: claim,
                _attached: attached,
            })),
        }
    }
}

/// Running pair with the Runner stdin channel retained and JIT withheld.
#[must_use]
pub(crate) struct StartedBootstrapPair {
    receipt: super::BootstrapStartReceipt,
    mount_handoff: super::VerifiedBootstrapMountHandoff,
    stdin: Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    output: Option<RunnerOutput>,
    stdin_open: bool,
    jit_withheld: bool,
    delivery_attempted: bool,
    delivery_succeeded: bool,
}

pub(crate) type RunnerOutput = Pin<
    Box<dyn Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send>,
>;

impl StartedBootstrapPair {
    pub(crate) fn receipt(&self) -> &super::BootstrapStartReceipt {
        &self.receipt
    }

    pub(crate) fn mount_handoff(&self) -> &super::VerifiedBootstrapMountHandoff {
        &self.mount_handoff
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        self.receipt.runner_container_id()
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        self.receipt.dind_container_id()
    }

    pub(crate) const fn runner_stdin_open(&self) -> bool {
        self.stdin_open
    }

    pub(crate) const fn jit_withheld(&self) -> bool {
        self.jit_withheld
    }

    pub(crate) fn take_output(&mut self) -> Option<RunnerOutput> {
        if !self.delivery_succeeded || self.stdin_open || self.jit_withheld {
            return None;
        }
        self.output.take()
    }

    /// Write one claimed JIT value and close Runner stdin before the deadline.
    pub(crate) async fn deliver_jit(
        &mut self,
        claim: &crate::journal::ListenerDeliveryClaim,
        jit: EncodedJit,
        deadline: Instant,
    ) -> Result<crate::launch::bootstrap::ListenerDeliveryReceipt, HostError> {
        let runner = claim.runner().clone();
        let bytes = jit.expose().as_bytes();
        crate::launch::bootstrap::ListenerDeliveryReceipt::validate_claim_runner(claim, &runner)?;
        if !self.runner_stdin_open()
            || !self.jit_withheld()
            || claim.reservation_id() != self.receipt.reservation_id()
            || claim.operation_id() != self.receipt.operation_id()
            || claim.engine_id() != self.receipt.engine_id()
            || claim.docker_root_digest() != self.receipt.docker_root_digest()
            || claim.runner_container_id() != self.receipt.runner_container_id()
            || !valid_jit_payload(bytes)
            || self.delivery_attempted
            || Instant::now() >= deadline
        {
            return Err(HostError::Identity);
        }
        self.delivery_attempted = true;
        let write = async {
            self.stdin
                .write_all(bytes)
                .await
                .map_err(|_| HostError::Docker)?;
            self.stdin.shutdown().await.map_err(|_| HostError::Docker)
        };
        tokio::time::timeout_at(deadline.into(), write)
            .await
            .map_err(|_| HostError::DockerTimeout)
            .and_then(|result| result)?;
        self.stdin_open = false;
        self.jit_withheld = false;
        self.delivery_succeeded = true;
        crate::launch::bootstrap::ListenerDeliveryReceipt::from_written_jit(
            claim,
            runner,
            bytes.len(),
            bytes.len(),
            true,
        )
    }
}

const JIT_MAX_BYTES: usize = 131_072 - b"ACTIONS_RUNNER_INPUT_JITCONFIG".len() - 2;

fn valid_jit_payload(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.is_ascii() && !bytes.contains(&0) && bytes.len() <= JIT_MAX_BYTES
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod failure_tests {
    use super::PairStartFailure;
    use crate::error::HostError;

    #[test]
    fn before_attach_failure_returns_error_without_pending_pair() {
        let failure = PairStartFailure::before_attach(HostError::Identity);

        let (error, pending) = failure.into_parts();

        assert!(matches!(error, HostError::Identity));
        assert!(pending.is_none());
    }

    #[test]
    fn pair_start_failure_has_bounded_inline_size() {
        assert!(std::mem::size_of::<PairStartFailure>() <= 128);
    }
}
