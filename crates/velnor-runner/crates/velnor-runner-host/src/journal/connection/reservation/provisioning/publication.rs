use velnor_runner_github::RunnerReference;

mod delivery;
mod job_facts;
#[cfg(test)]
mod test_snapshot;
mod validation;

/// One durable publication attempt for the read-only job-facts file.
pub(crate) struct JobFactsPublicationClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner: RunnerReference,
    instance_binding_digest: String,
    profile_generation_id: String,
    profile_digest: String,
    sample_generation_id: String,
}

impl JobFactsPublicationClaim {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
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

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }
}

/// One durable JIT delivery attempt after publication is recorded.
pub(crate) struct ListenerDeliveryClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    runner: RunnerReference,
}

impl ListenerDeliveryClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }
}

struct BoundState {
    bootstrap_state: String,
    resources_bound: bool,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    jit_state: String,
    publication_state: String,
    delivery_state: String,
    cleanup_proven: bool,
}
