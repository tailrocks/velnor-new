use std::io;

use velnor_resource_probe::{
    ProbeError,
    worker_binding::{
        OwnedCgroupRequest, SelfStorageRequest, WORKER_REQUEST_LIMIT, observe_owned_cgroups,
        observe_self_storage,
    },
};

#[cfg(target_os = "linux")]
use super::write_worker_record_limit;
use super::{main_ack::wait_for_ack, read_worker_request, write_worker_record};

#[cfg(target_os = "linux")]
pub(super) fn run_execution_channel() -> Result<(), ProbeError> {
    use velnor_resource_probe::execution_channel::{
        ExecutionChannelContext, observe_execution_channel,
    };

    let request: ExecutionChannelContext = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = observe_execution_channel(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

#[cfg(target_os = "linux")]
pub(super) fn run_execution_request() -> Result<(), ProbeError> {
    use velnor_resource_probe::execution_channel::{
        ExecutionRequestContext, read_execution_request,
    };

    let request: ExecutionRequestContext = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let outcome = read_execution_request(request)?;
    write_worker_record(&outcome, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

#[cfg(target_os = "linux")]
pub(super) fn run_execution_publication() -> Result<(), ProbeError> {
    use velnor_resource_probe::execution_publication::{
        ExecutionPublicationContext, PUBLICATION_REQUEST_LIMIT, publish_execution_control,
    };

    let request: ExecutionPublicationContext = read_worker_request(PUBLICATION_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = publish_execution_control(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

#[cfg(target_os = "linux")]
pub(super) fn run_job_facts_publication() -> Result<(), ProbeError> {
    use velnor_resource_probe::{
        job_facts::{JOB_FACTS_RECORD_LIMIT, JOB_FACTS_REQUEST_LIMIT, publish_job_facts},
        job_facts_wire::JobFactsPublicationContext,
    };

    let request: JobFactsPublicationContext = read_worker_request(JOB_FACTS_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = publish_job_facts(request)?;
    write_worker_record_limit(&record, &mut io::stdout().lock(), JOB_FACTS_RECORD_LIMIT)?;
    wait_for_ack(&operation_id)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_execution_channel() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_execution_request() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_execution_publication() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_job_facts_publication() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(target_os = "linux")]
pub(super) fn run_cleanup_observation() -> Result<(), ProbeError> {
    use velnor_resource_probe::cleanup_observation::{CleanupObservationRequest, observe_cleanup};

    let request: CleanupObservationRequest = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = observe_cleanup(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

#[cfg(target_os = "linux")]
pub(super) fn run_cleanup_observer() -> Result<(), ProbeError> {
    use velnor_resource_probe::cleanup_observer::{
        CLEANUP_REQUEST_LIMIT, CleanupObserverRequest, observe_cleanup,
    };

    let request: CleanupObserverRequest = read_worker_request(CLEANUP_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = observe_cleanup(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_cleanup_observer() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_cleanup_observation() -> Result<(), ProbeError> {
    Err(ProbeError::Invalid("linux_guest_required"))
}

pub(super) fn run_self_storage() -> Result<(), ProbeError> {
    let request: SelfStorageRequest = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let operation_id = request.operation_id.clone();
    let record = observe_self_storage(request)?;
    write_worker_record(&record, &mut io::stdout().lock())?;
    wait_for_ack(&operation_id)
}

pub(super) fn run_owned_cgroup() -> Result<(), ProbeError> {
    let request: OwnedCgroupRequest = read_worker_request(WORKER_REQUEST_LIMIT)?;
    let record = observe_owned_cgroups(&request)?;
    write_worker_record(&record, &mut io::stdout().lock())
}
