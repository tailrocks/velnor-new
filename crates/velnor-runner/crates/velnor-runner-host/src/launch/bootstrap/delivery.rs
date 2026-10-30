use velnor_runner_github::RunnerReference;

/// Proof that JIT bytes were fully written to the retained runner stdin and
/// that the input stream was then closed.
#[must_use]
pub(crate) struct ListenerDeliveryReceipt {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    runner: RunnerReference,
    jit_bytes_written: bool,
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
        self.jit_bytes_written
    }

    pub(crate) const fn stdin_closed(&self) -> bool {
        self.stdin_closed
    }

    pub(crate) const fn delivered(&self) -> bool {
        self.jit_bytes_written && self.stdin_closed
    }
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
        jit_bytes_written: true,
        stdin_closed: true,
    })
}
