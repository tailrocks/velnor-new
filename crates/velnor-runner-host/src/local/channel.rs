// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::query_parameters::{AttachContainerOptionsBuilder, DownloadFromContainerOptions};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use velnor_resource_probe::job_hook::{Challenge, Decision, canonical_json, sha256};

use crate::docker_client::docker_deadline;
use crate::error::HostError;

use super::io::{MetadataUpload, upload_to_metadata};
use super::layout;
use super::pair::LocalPair;

const CHALLENGE_LIMIT: usize = 8 * 1024;
const DECISION_LIMIT: usize = 8 * 1024;
const MAX_REQUEST_ARCHIVE_BYTES: usize = 64 * 1024;
const MAX_JIT_BYTES: usize = 131_072 - b"ACTIONS_RUNNER_INPUT_JITCONFIG".len() - 2;
pub(super) const REQUEST_POLL: Duration = Duration::from_millis(100);
pub(super) const POLICY_GENERATION: u64 = 1;
pub(super) const SCALE_SET_ID: i64 = 1;

pub(super) fn challenge(pair: &LocalPair, sequence: u32) -> Result<Challenge, HostError> {
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| HostError::Identity)?;
    let id = i64::from(sequence);
    let digest = |part: &str| sha256(format!("velnor-local:{}:{part}", pair.name()).as_bytes());
    Ok(Challenge {
        schema_version: 1,
        nonce: crate::digest_hex::lowercase_hex(&nonce),
        reservation_id: id,
        intent_id: id,
        witness_request_id: id,
        runner_id: id,
        runner_name: pair.request().agent.clone(),
        runner_scale_set_id: SCALE_SET_ID,
        runner_container_id: pair.runner_id().to_owned(),
        dind_container_id: pair.dind_id().to_owned(),
        engine_id: pair.environment().to_owned(),
        docker_root_digest: digest("docker-root"),
        instance_binding_digest: digest("instance"),
        policy_generation: POLICY_GENERATION,
        policy_digest: sha256(b"velnor-local:no-policy"),
        channel_digest: digest("channel"),
    })
}

pub(super) async fn publish_challenge(
    docker: &Docker,
    pair: &LocalPair,
    challenge: &Challenge,
) -> Result<(), HostError> {
    let bytes = canonical_json(challenge, CHALLENGE_LIMIT, "local_challenge")
        .map_err(|_| HostError::Identity)?;
    publish(docker, pair, "challenge.json", &bytes).await
}

pub(super) async fn publish_decision(
    docker: &Docker,
    pair: &LocalPair,
    decision: &Decision,
) -> Result<(), HostError> {
    let bytes = canonical_json(decision, DECISION_LIMIT, "local_decision")
        .map_err(|_| HostError::Identity)?;
    publish(docker, pair, "decision.json", &bytes).await
}

async fn publish(
    docker: &Docker,
    pair: &LocalPair,
    name: &str,
    bytes: &[u8],
) -> Result<(), HostError> {
    upload_to_metadata(
        docker,
        pair.request(),
        pair.name(),
        pair.metadata_volume(),
        MetadataUpload::Control { name, bytes },
    )
    .await
}

pub(super) async fn read_request(
    docker: &Docker,
    pair: &LocalPair,
) -> Result<Option<Vec<u8>>, HostError> {
    let options = DownloadFromContainerOptions {
        path: layout::INBOX_REQUEST.to_owned(),
    };
    let mut stream = docker.download_from_container(pair.runner_id(), Some(options));
    let mut archive = Vec::new();
    while let Some(chunk) = docker_deadline(stream.next()).await? {
        match chunk {
            Ok(chunk) => archive.extend_from_slice(&chunk),
            Err(DockerError::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(None),
            Err(_) => return Err(HostError::Docker),
        }
        if archive.len() > MAX_REQUEST_ARCHIVE_BYTES {
            return Err(HostError::Frame);
        }
    }
    request_from_archive(&archive).map(Some)
}

fn request_from_archive(archive: &[u8]) -> Result<Vec<u8>, HostError> {
    let mut archive = tar::Archive::new(archive);
    let mut entries = archive.entries().map_err(|_| HostError::Identity)?;
    let mut entry = entries
        .next()
        .ok_or(HostError::Identity)?
        .map_err(|_| HostError::Identity)?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_REQUEST_ARCHIVE_BYTES).map_err(|_| HostError::Identity)?;
    entry
        .by_ref()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Identity)?;
    Ok(bytes)
}

pub(super) async fn deliver_jit(
    docker: &Docker,
    pair: &LocalPair,
    jit: &str,
) -> Result<(), HostError> {
    let bytes = jit.as_bytes();
    if bytes.is_empty() || !bytes.is_ascii() || bytes.contains(&0) || bytes.len() > MAX_JIT_BYTES {
        return Err(HostError::EmptyJit);
    }
    let options = AttachContainerOptionsBuilder::default()
        .stdin(true)
        .stdout(false)
        .stderr(false)
        .stream(true)
        .logs(false)
        .build();
    let mut attached = docker_deadline(docker.attach_container(pair.runner_id(), Some(options)))
        .await?
        .map_err(|_| HostError::Docker)?;
    let write = async {
        attached.input.write_all(bytes).await?;
        attached.input.shutdown().await
    };
    docker_deadline(write)
        .await?
        .map_err(|_| HostError::JitDeliveryUncertain)?;
    Ok(())
}
