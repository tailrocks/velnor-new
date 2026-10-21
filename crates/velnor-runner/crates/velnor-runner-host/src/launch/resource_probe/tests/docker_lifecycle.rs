use serde_json::json;

use crate::error::HostError;
use crate::journal::{Journal, ProbeSeed, ResourceProbePurpose};
use crate::launch::inspect_tests::{DockerResponse, DockerStub, closed, delayed_http, http};
use crate::launch::resource_probe::execute;
use crate::launch::resource_probe::lifecycle::{self, Deadline};
use crate::launch::resource_probe::projection::{
    ProbeProjection, VerifiedProbeImage, expected_image_config,
};
use crate::launch::resource_probe::provider::ProbeImagePreparationEvidence;
use crate::launch::resource_probe::provider::UnavailableImageProvider;
use crate::launch_harness::Scratch;
use std::time::Duration;

#[path = "docker_lifecycle/effective_config.rs"]
mod effective_config;
#[path = "docker_lifecycle/engine_identity.rs"]
mod engine_identity;
#[path = "docker_lifecycle/execution.rs"]
mod execution;
#[path = "docker_lifecycle/readiness_guard.rs"]
mod readiness_guard;
#[path = "docker_lifecycle/readiness_poll.rs"]
pub(crate) mod readiness_poll;
#[path = "docker_lifecycle/recovery.rs"]
mod recovery;

const CONTAINER_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OPERATION_ID: &str = "dddddddddddddddddddddddddddddddd";
const ENGINE_ID: &str = "probe-engine";
const ROOT: &str = "/var/lib/docker";
const REVISION: &str = "b7c3309d33451f8e256a7e255239e9dc9285f042";
const BINDING: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RUNTIME_ID: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SELECTED_INFO: &str = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64"}"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestKind {
    Info,
    Create,
    Inspect,
    Start,
    Wait,
    Logs,
    Remove,
    Other,
}

async fn prepared() -> Result<(Scratch, Journal, ProbeProjection), String> {
    let scratch = Scratch::new("probe-lifecycle").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    super::configure_probe_capacity(&journal).await?;
    let instance_id = journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    let projection = ProbeProjection::build(
        "d".repeat(32),
        instance_id.clone(),
        ENGINE_ID.to_owned(),
        ROOT,
        VerifiedProbeImage::from_verified_provider(
            RUNTIME_ID.to_owned(),
            "linux/amd64".to_owned(),
            "x86_64".to_owned(),
            REVISION.to_owned(),
            BINDING.to_owned(),
            &expected_image_config(REVISION),
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    journal
        .prepare_probe(ProbeSeed {
            operation_id: projection.operation_id.clone(),
            instance_id,
            engine_id: projection.engine_id.clone(),
            docker_root_digest: projection.root.digest().to_owned(),
            source_revision: projection.image.source_revision().to_owned(),
            runtime_image_id: projection.image.runtime_id().to_owned(),
            image_binding_digest: projection.image.binding_fingerprint().to_owned(),
            operation_name: projection.name.clone(),
            projection_digest: projection.projection_digest.clone(),
            purpose: ResourceProbePurpose::Capacity,
            request_sha256: projection.request_sha256.clone(),
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal, projection))
}

fn image_evidence(projection: &ProbeProjection) -> ProbeImagePreparationEvidence {
    ProbeImagePreparationEvidence::test_fixture(
        &projection.engine_id,
        projection.root.digest(),
        projection.image.daemon_architecture(),
        projection.image.source_revision(),
    )
}

fn start_responses(projection: &ProbeProjection) -> Result<Vec<DockerResponse>, String> {
    start_responses_for_info(projection, SELECTED_INFO)
}

fn start_responses_for_info(
    projection: &ProbeProjection,
    info: &str,
) -> Result<Vec<DockerResponse>, String> {
    Ok(vec![
        http(200, info),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, info),
        http(200, &inspect(projection, "created", 0)?),
        http(200, &inspect(projection, "created", 0)?),
        http(200, info),
    ])
}

fn start_request_order() -> Vec<RequestKind> {
    vec![
        RequestKind::Info,
        RequestKind::Create,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
    ]
}

fn timeout_request_order() -> Vec<RequestKind> {
    let mut expected = start_request_order();
    expected.extend([
        RequestKind::Start,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Wait,
        RequestKind::Inspect,
        RequestKind::Logs,
        RequestKind::Info,
        RequestKind::Info,
    ]);
    expected
}

fn cleanup_request_order() -> Vec<RequestKind> {
    vec![
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
        RequestKind::Remove,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
    ]
}

fn recovery_request_order() -> Vec<RequestKind> {
    vec![
        RequestKind::Info,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
        RequestKind::Remove,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Inspect,
        RequestKind::Info,
    ]
}

fn assert_start_rebind_blocks_effects(requests: &[String]) -> Result<(), String> {
    assert_request_order(
        requests,
        &[
            RequestKind::Info,
            RequestKind::Create,
            RequestKind::Info,
            RequestKind::Inspect,
            RequestKind::Inspect,
            RequestKind::Info,
            RequestKind::Info,
        ],
    )?;
    if requests.iter().any(|request| {
        request.starts_with(&format!("POST /containers/{CONTAINER_ID}/start?"))
            || request.starts_with(&format!("POST /containers/{CONTAINER_ID}/stop?"))
            || request.starts_with(&format!("DELETE /containers/{CONTAINER_ID}?"))
    }) {
        return Err(format!("rebound engine received an effect: {requests:?}"));
    }
    Ok(())
}

fn assert_recovered_container_cleanup(requests: &[String]) -> Result<(), String> {
    assert_request_order(requests, &recovery_request_order())
}

fn assert_request_order(requests: &[String], expected: &[RequestKind]) -> Result<(), String> {
    let actual = requests
        .iter()
        .map(|request| request_kind(request))
        .collect::<Vec<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "unexpected Docker request order: {actual:?}; requests: {requests:?}"
        ))
    }
}

fn request_kind(request: &str) -> RequestKind {
    let mut parts = request.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return RequestKind::Other;
    };
    let path = target.split('?').next().unwrap_or(target);
    match (method, path) {
        ("GET", "/info") => RequestKind::Info,
        ("POST", "/containers/create") => RequestKind::Create,
        ("GET", path) if path.starts_with("/containers/") && path.ends_with("/json") => {
            RequestKind::Inspect
        }
        ("POST", path) if path == format!("/containers/{CONTAINER_ID}/start") => RequestKind::Start,
        ("POST", path) if path == format!("/containers/{CONTAINER_ID}/wait") => RequestKind::Wait,
        ("GET", path) if path == format!("/containers/{CONTAINER_ID}/logs") => RequestKind::Logs,
        ("DELETE", path) if path == format!("/containers/{CONTAINER_ID}") => RequestKind::Remove,
        _ => RequestKind::Other,
    }
}

fn append_cleanup(
    responses: &mut Vec<DockerResponse>,
    projection: &ProbeProjection,
    state: &str,
    exit_code: i64,
    first_info_delay: Duration,
) -> Result<(), String> {
    append_cleanup_for_info(
        responses,
        projection,
        state,
        exit_code,
        first_info_delay,
        SELECTED_INFO,
    )
}

fn append_cleanup_for_info(
    responses: &mut Vec<DockerResponse>,
    projection: &ProbeProjection,
    state: &str,
    exit_code: i64,
    first_info_delay: Duration,
    info: &str,
) -> Result<(), String> {
    let absent = "{}";
    responses.push(delayed_http(200, info, first_info_delay));
    responses.extend([
        http(200, &inspect(projection, state, exit_code)?),
        http(200, &inspect(projection, state, exit_code)?),
        http(200, info),
        http(200, "{}"),
        http(200, info),
        http(404, absent),
        http(404, absent),
        http(404, absent),
        http(404, absent),
        http(200, info),
        http(404, absent),
        http(404, absent),
        http(200, info),
    ]);
    Ok(())
}

fn inspect(projection: &ProbeProjection, status: &str, exit_code: i64) -> Result<String, String> {
    let mut labels = std::collections::HashMap::from([(
        "org.opencontainers.image.revision".to_owned(),
        projection.image.source_revision().to_owned(),
    )]);
    labels.extend(projection.config.labels.clone().unwrap_or_default());
    let config = json!({
        "Hostname": &CONTAINER_ID[..12],
        "User": "65532:65532",
        "AttachStdin": false,
        "AttachStdout": true,
        "AttachStderr": false,
        "Tty": false,
        "OpenStdin": false,
        "StdinOnce": false,
        "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
        "Image": projection.image.runtime_id(),
        "WorkingDir": "/",
        "Entrypoint": ["/velnor/resource-probe"],
        "NetworkDisabled": true,
        "Labels": labels,
    });
    let host_config = projection
        .config
        .host_config
        .as_ref()
        .ok_or_else(|| "projection host config is missing".to_owned())?;
    let response = json!({
        "Id": CONTAINER_ID,
        "Name": format!("/{}", projection.name),
        "Image": projection.image.runtime_id(),
        "Path": "/velnor/resource-probe",
        "Args": [],
        "RestartCount": 0,
        "State": {
            "Status": status,
            "Running": status == "running",
            "Paused": false,
            "Restarting": false,
            "OOMKilled": false,
            "Dead": false,
            "ExitCode": exit_code,
            "Pid": if status == "running" { 1234 } else { 0 }
        },
        "Config": config,
        "HostConfig": host_config,
        "Mounts": [
            {
                "Type": "bind",
                "Source": projection.root.path(),
                "Destination": "/velnor/docker-root",
                "RW": false
            },
            {
                "Type": "bind",
                "Source": crate::launch::resource_probe::projection::CGROUP_TARGET,
                "Destination": crate::launch::resource_probe::projection::CGROUP_TARGET,
                "RW": false
            }
        ]
    });
    serde_json::to_string(&response).map_err(|error| error.to_string())
}

fn record_bytes() -> Vec<u8> {
    let mut record = super::super::record::fixture_record();
    record.docker_root_free_bytes = 32 * 1024 * 1024 * 1024;
    record.docker_root_available_bytes = 30 * 1024 * 1024 * 1024;
    record.memory_available_bytes = 10 * 1024 * 1024 * 1024;
    let mut bytes = serde_json::to_vec(&record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn log_frame(record: &[u8]) -> Result<Vec<u8>, String> {
    let length = u32::try_from(record.len()).map_err(|error| error.to_string())?;
    let mut frame = vec![1, 0, 0, 0];
    frame.extend(length.to_be_bytes());
    frame.extend_from_slice(record);
    Ok(frame)
}
