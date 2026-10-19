use velnor_runner_github::RunnerReference;

use crate::error::HostError;
use crate::journal::ListenerDeliveryClaim;

/// Proof that the controller wrote all JIT bytes to runner stdin and then closed the input stream.
#[must_use]
pub(crate) struct ListenerDeliveryReceipt {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    runner: RunnerReference,
    jit_bytes_written: usize,
    stdin_closed: bool,
}

impl ListenerDeliveryReceipt {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) const fn jit_bytes_written(&self) -> bool {
        self.jit_bytes_written > 0
    }

    pub(crate) const fn stdin_closed(&self) -> bool {
        self.stdin_closed
    }

    pub(crate) const fn delivered(&self) -> bool {
        self.jit_bytes_written > 0 && self.stdin_closed
    }

    /// Create proof after the exact claim's JIT bytes reach Runner stdin and stdin closes.
    pub(super) fn from_written_jit(
        claim: &ListenerDeliveryClaim,
        runner: RunnerReference,
        bytes_written: usize,
        expected_bytes: usize,
        stdin_closed: bool,
    ) -> Result<Self, HostError> {
        Self::validate_claim_runner(claim, &runner)?;
        if bytes_written == 0 || bytes_written != expected_bytes || !stdin_closed {
            return Err(HostError::Identity);
        }
        Ok(Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            runner_container_id: claim.runner_container_id().to_owned(),
            runner,
            jit_bytes_written: bytes_written,
            stdin_closed,
        })
    }

    pub(super) fn validate_claim_runner(
        claim: &ListenerDeliveryClaim,
        runner: &RunnerReference,
    ) -> Result<(), HostError> {
        let claimed_runner = claim.runner();
        if claim.reservation_id() <= 0
            || claim.operation_id() <= 0
            || !valid_engine_id(claim.engine_id())
            || !valid_digest(claim.docker_root_digest())
            || !valid_container_id(claim.runner_container_id())
            || runner.id <= 0
            || runner.runner_scale_set_id <= 0
            || runner.name.len() != 33
            || !runner.name.starts_with('v')
            || !runner.name[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || runner.id != claimed_runner.id
            || runner.runner_scale_set_id != claimed_runner.runner_scale_set_id
            || runner.name != claimed_runner.name
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

fn valid_engine_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
pub(crate) fn test_listener_delivery_receipt(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    runner: RunnerReference,
) -> Result<ListenerDeliveryReceipt, crate::error::HostError> {
    use crate::launch::bootstrap::test_fixtures::{
        valid_container_id, valid_digest, valid_engine_id,
    };

    let valid_runner = runner.id > 0
        && runner.runner_scale_set_id > 0
        && runner.name.len() == 33
        && runner.name.starts_with('v')
        && runner.name[1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || !valid_container_id(&runner_container_id)
        || !valid_runner
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(ListenerDeliveryReceipt {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        runner_container_id,
        runner,
        jit_bytes_written: 1,
        stdin_closed: true,
    })
}
