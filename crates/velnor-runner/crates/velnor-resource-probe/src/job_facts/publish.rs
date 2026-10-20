use std::fs::File;

use super::{JOB_FACTS_RECORD_LIMIT, JOB_FACTS_REQUEST_LIMIT};
use rustix::process::geteuid;

use crate::{
    ProbeError,
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::{
        JobFactsPublicationContext, JobFactsPublicationFile, JobFactsPublicationRecord,
    },
    mountinfo::{MountEntry, attachment_mount, attachment_mount_source},
};

const ROOT_TARGET: &str = "/opt/velnor";
const DIRECTORY_TARGET: &str = "/opt/velnor/job-facts";
const FILE_NAME: &str = "storage-cache-admission.json";
const FILE_PATH: &str = "/opt/velnor/job-facts/storage-cache-admission.json";
const RUNTIME_DIRECTORY_TARGET: &str = "/opt/velnor/runtime-identity";
const GUEST_IMAGE_NAME: &str = "guest-image.json";
const GUEST_IMAGE_PATH: &str = "/opt/velnor/runtime-identity/guest-image.json";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MOUNTINFO_LIMIT: usize = 64 * 1024;
const PAYLOAD_LIMIT: usize = 64 * 1024;
const GUEST_IMAGE_LIMIT: usize = 16 * 1024;
const ROOT_UID: u32 = 0;
const ROOT_GID: u32 = 0;
const DIRECTORY_MODE: u32 = 0o555;
const FILE_MODE: u32 = 0o444;
const BTRFS_MAGIC: u64 = 0x9123_683e;

mod directories;
mod files;
mod validation;
use directories::{
    compare_runner_directory, compare_runtime_directory, observe_directory, open_directory,
};
use files::{
    StableFileState, inspect_file, publish_staged, read_mountinfo, stage_file, verify_stable_state,
};
#[cfg(test)]
use validation::valid_document_shape;
use validation::{decode_payload, lower_hex, validate_context};

#[cfg(test)]
mod tests;

pub(super) fn publish(
    context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    validate_context(&context)?;
    validate_request_size(&context)?;
    if geteuid().as_raw() != ROOT_UID {
        return Err(ProbeError::Invalid("job_facts_root_required"));
    }
    publish_validated_request(context)
}

fn validate_request_size(context: &JobFactsPublicationContext) -> Result<(), ProbeError> {
    let size = serde_json::to_vec(context)
        .map_err(|_| ProbeError::Invalid("job_facts_request"))?
        .len();
    if size > JOB_FACTS_REQUEST_LIMIT {
        return Err(ProbeError::InputTooLarge("job_facts_request"));
    }
    Ok(())
}

fn publish_validated_request(
    context: JobFactsPublicationContext,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    let bytes = decode_payload(&context.payload_hex)?;
    let guest_image = validation::decode_guest_image(&context.guest_image_hex)?;
    let boot_id = crate::execution_channel::identity::read_boot_id()?;
    if boot_id != context.expected_guest_boot_id {
        return Err(ProbeError::Invalid("job_facts_boot_id"));
    }
    let namespace = crate::execution_channel::identity::mount_namespace_id()?;
    publish_on_mount(context, &bytes, &guest_image, boot_id, namespace)
}

fn publish_on_mount(
    context: JobFactsPublicationContext,
    bytes: &[u8],
    guest_image: &[u8],
    boot_id: String,
    namespace: u64,
) -> Result<JobFactsPublicationRecord, ProbeError> {
    let mountinfo = read_mountinfo()?;
    let mount = attachment_mount(&mountinfo, ROOT_TARGET)?;
    if mount.read_only {
        return Err(ProbeError::Invalid("job_facts_mount_read_only"));
    }
    let source = attachment_mount_source(&mountinfo, ROOT_TARGET)?;
    let directory = open_directory("job-facts")?;
    let runtime_directory = open_directory("runtime-identity")?;
    let observed = observe_directory(&directory, &mount, &source, DIRECTORY_TARGET)?;
    let runtime_observed = observe_directory(
        &runtime_directory,
        &mount,
        &source,
        RUNTIME_DIRECTORY_TARGET,
    )?;
    compare_runner_directory(&context.expected_runner_job_facts, &observed)?;
    compare_runtime_directory(&context.expected_runner_runtime_identity, &runtime_observed)?;
    let (file, guest_image_file) = publish_and_verify(&PairPublication {
        operation_id: &context.operation_id,
        directories: [&directory, &runtime_directory],
        mountinfo: &mountinfo,
        mount: &mount,
        observations: [&observed, &runtime_observed],
        documents: [bytes, guest_image],
    })?;
    let output = JobFactsPublicationRecord {
        schema_version: 2,
        operation_id: context.operation_id,
        guest_boot_id: boot_id,
        mount_namespace_id: namespace,
        payload_sha256: context.payload_sha256,
        guest_image_sha256: context.guest_image_sha256,
        publication_binding_sha256: context.publication_binding_sha256,
        directory: observed,
        runtime_identity_directory: runtime_observed,
        file,
        guest_image_file,
    };
    validate_record_size(&output)?;
    Ok(output)
}

struct PairPublication<'a> {
    operation_id: &'a str,
    directories: [&'a File; 2],
    mountinfo: &'a [u8],
    mount: &'a MountEntry,
    observations: [&'a ExecutionChannelDirectory; 2],
    documents: [&'a [u8]; 2],
}

fn publish_and_verify(
    pair: &PairPublication<'_>,
) -> Result<(JobFactsPublicationFile, JobFactsPublicationFile), ProbeError> {
    let [facts_directory, image_directory] = pair.directories;
    let [facts, image] = pair.documents;
    publish_pair(
        facts_directory,
        image_directory,
        pair.operation_id,
        FILE_NAME,
        facts,
        GUEST_IMAGE_NAME,
        image,
    )?;
    let facts_file = inspect_file(
        facts_directory,
        FILE_NAME,
        FILE_PATH,
        facts,
        pair.mount,
        PAYLOAD_LIMIT,
    )?;
    let image_file = inspect_file(
        image_directory,
        GUEST_IMAGE_NAME,
        GUEST_IMAGE_PATH,
        image,
        pair.mount,
        GUEST_IMAGE_LIMIT,
    )?;
    verify_stable_state(&StableFileState {
        directory: facts_directory,
        mountinfo: pair.mountinfo,
        mount: pair.mount,
        observed: pair.observations[0],
        file: &facts_file,
        name: FILE_NAME,
        target: DIRECTORY_TARGET,
        limit: PAYLOAD_LIMIT,
    })?;
    verify_stable_state(&StableFileState {
        directory: image_directory,
        mountinfo: pair.mountinfo,
        mount: pair.mount,
        observed: pair.observations[1],
        file: &image_file,
        name: GUEST_IMAGE_NAME,
        target: RUNTIME_DIRECTORY_TARGET,
        limit: GUEST_IMAGE_LIMIT,
    })?;
    Ok((facts_file, image_file))
}

fn validate_record_size(record: &JobFactsPublicationRecord) -> Result<(), ProbeError> {
    let size = serde_json::to_vec(record)
        .map_err(|_| ProbeError::Invalid("job_facts_record"))?
        .len();
    if size > JOB_FACTS_RECORD_LIMIT {
        return Err(ProbeError::InputTooLarge("job_facts_record"));
    }
    Ok(())
}

fn publish_pair(
    facts_directory: &File,
    runtime_directory: &File,
    operation_id: &str,
    facts_name: &str,
    facts: &[u8],
    image_name: &str,
    image: &[u8],
) -> Result<(), ProbeError> {
    let facts_temp = stage_file(facts_directory, operation_id, facts_name, facts)?;
    let image_temp = stage_file(runtime_directory, operation_id, image_name, image)?;
    publish_staged(facts_directory, &facts_temp, facts_name)?;
    publish_staged(runtime_directory, &image_temp, image_name)?;
    Ok(())
}
