use serde_json::json;

use crate::error::HostError;
use crate::journal::{Journal, ProbeSeed};
use crate::launch::inspect_tests::{
    DockerResponse, DockerStub, closed, delayed_http, http, raw_http,
};
use crate::launch::resource_probe::execute;
use crate::launch::resource_probe::lifecycle::{self, Deadline};
use crate::launch::resource_probe::projection::{
    ProbeProjection, VerifiedProbeImage, expected_image_config,
};
use crate::launch::resource_probe::provider::UnavailableImageProvider;
use crate::launch_harness::Scratch;
use std::time::{Duration, Instant};

#[path = "docker_lifecycle/effective_config.rs"]
mod effective_config;
#[path = "docker_lifecycle/engine_identity.rs"]
mod engine_identity;
#[path = "docker_lifecycle/recovery.rs"]
mod recovery;

const CONTAINER_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENGINE_ID: &str = "probe-engine";
const ROOT: &str = "/var/lib/docker";
const REVISION: &str = "b7c3309d33451f8e256a7e255239e9dc9285f042";
const BINDING: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RUNTIME_ID: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test]
async fn completed_probe_cleans_the_exact_container_before_returning_metrics() -> Result<(), String>
{
    let (scratch, journal, projection) = prepared().await?;
    let cleanup_delay = Duration::from_millis(80);
    let mut responses = start_responses(&projection)?;
    responses.push(http(200, "{}"));
    responses.push(http(200, r#"{"StatusCode":0,"Error":null}"#));
    responses.push(http(200, &inspect(&projection, "exited", 0)?));
    responses.push(raw_http(200, log_frame(&record_bytes())?));
    append_cleanup(&mut responses, &projection, "exited", 0, cleanup_delay)?;
    let docker = DockerStub::open(responses)?;

    let output = execute::run(
        &docker.docker,
        &journal,
        &projection,
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    )
    .await;
    let requests = docker.finish_observed().await?;
    let output = output.map_err(|error| format!("{error}; requests: {requests:?}"))?;
    let observed_at = output.observed_at;
    assert!(observed_at.elapsed() >= cleanup_delay);

    assert_eq!(requests.len(), 20);
    assert!(requests[1].starts_with("POST /containers/create?"));
    assert!(requests[5].ends_with("/start HTTP/1.1"));
    assert!(requests[6].contains("/wait?"));
    assert!(requests[8].contains("/logs?"));
    assert!(requests[13].starts_with("DELETE /containers/"));
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    let budget = crate::worker::test_resource_budget().map_err(|error| error.to_string())?;
    let postcheck_delay = Duration::from_millis(80);
    tokio::time::sleep(postcheck_delay).await;
    let observation = lifecycle::observation(
        output,
        8,
        16 * 1024 * 1024 * 1024,
        ENGINE_ID.to_owned(),
        projection.root.digest().to_owned(),
    )
    .ok_or_else(|| "valid probe result was rejected".to_owned())?;
    assert_eq!(observation.observed_at(), observed_at);
    let final_check = Instant::now();
    assert!(final_check.duration_since(observed_at) >= cleanup_delay + postcheck_delay);
    let expired_at = observed_at
        .checked_add(Duration::from_secs(31))
        .ok_or_else(|| "could not construct expired output time".to_owned())?;
    assert!(!observation.is_recent_at(expired_at));
    assert!(observation.into_start_permit(budget).is_some());
    crate::launch_harness::absent(&scratch.file())
}

#[tokio::test]
async fn lost_or_invalid_execution_results_cleanup_without_a_permit() -> Result<(), String> {
    for failure in [
        Failure::StartLost,
        Failure::WaitLost,
        Failure::WaitNonzero,
        Failure::LogsLost,
        Failure::InvalidRecord,
    ] {
        let (scratch, journal, projection) = prepared().await?;
        let mut responses = start_responses(&projection)?;
        let cleanup_state = match failure {
            Failure::StartLost => {
                responses.push(closed());
                responses.push(http(200, &inspect(&projection, "created", 0)?));
                "created"
            }
            Failure::WaitLost => {
                responses.push(http(200, "{}"));
                responses.push(closed());
                "exited"
            }
            Failure::WaitNonzero => {
                responses.push(http(200, "{}"));
                responses.push(http(200, r#"{"StatusCode":9,"Error":null}"#));
                "exited"
            }
            Failure::LogsLost => {
                responses.push(http(200, "{}"));
                responses.push(http(200, r#"{"StatusCode":0,"Error":null}"#));
                responses.push(http(200, &inspect(&projection, "exited", 0)?));
                responses.push(closed());
                "exited"
            }
            Failure::InvalidRecord => {
                responses.push(http(200, "{}"));
                responses.push(http(200, r#"{"StatusCode":0,"Error":null}"#));
                responses.push(http(200, &inspect(&projection, "exited", 0)?));
                responses.push(raw_http(200, log_frame(b"{}\n")?));
                "exited"
            }
        };
        append_cleanup(
            &mut responses,
            &projection,
            cleanup_state,
            9,
            Duration::ZERO,
        )?;
        let docker = DockerStub::open(responses)?;

        assert!(
            execute::run(
                &docker.docker,
                &journal,
                &projection,
                16 * 1024 * 1024 * 1024,
                &Deadline::new(),
            )
            .await
            .is_err(),
            "failure case {failure:?} unexpectedly yielded metrics"
        );
        let requests = docker.finish_observed().await?;
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("DELETE "))
        );
        assert!(
            journal
                .active_probe()
                .await
                .map_err(|error| error.to_string())?
                .is_none()
        );
        crate::launch_harness::absent(&scratch.file())?;
    }
    Ok(())
}

#[tokio::test]
async fn prepared_restart_aborts_and_unverified_provider_stays_closed() -> Result<(), String> {
    let (scratch, journal, _projection) = prepared().await?;
    drop(journal);
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let info =
        r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":8589934592}"#;
    let docker = DockerStub::open(vec![http(200, info)])?;
    let sample = Box::pin(lifecycle::collect(
        &docker.docker,
        &journal,
        &UnavailableImageProvider,
    ))
    .await
    .map_err(|error| error.to_string())?;
    let requests = docker.finish().await?;

    assert!(sample.is_none());
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET ") && requests[0].contains("/info"),
        "unexpected request: {requests:?}"
    );
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[derive(Debug)]
enum Failure {
    StartLost,
    WaitLost,
    WaitNonzero,
    LogsLost,
    InvalidRecord,
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
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal, projection))
}

fn start_responses(projection: &ProbeProjection) -> Result<Vec<DockerResponse>, String> {
    Ok(vec![
        http(
            200,
            r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#,
        ),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, &inspect(projection, "created", 0)?),
        http(200, &inspect(projection, "created", 0)?),
        http(
            200,
            r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker"}"#,
        ),
    ])
}

fn append_cleanup(
    responses: &mut Vec<DockerResponse>,
    projection: &ProbeProjection,
    state: &str,
    exit_code: i64,
    first_info_delay: Duration,
) -> Result<(), String> {
    let info = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker"}"#;
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
            "Running": false,
            "Paused": false,
            "Restarting": false,
            "OOMKilled": false,
            "Dead": false,
            "ExitCode": exit_code
        },
        "Config": config,
        "HostConfig": host_config,
        "Mounts": [{
            "Type": "bind",
            "Source": projection.root.path(),
            "Destination": "/velnor/docker-root",
            "RW": false
        }]
    });
    serde_json::to_string(&response).map_err(|error| error.to_string())
}

fn record_bytes() -> Vec<u8> {
    format!(
        "{{\"schema_version\":1,\"docker_root_free_bytes\":{},\"docker_root_total_bytes\":{},\"memory_available_bytes\":{},\"load_milli\":125,\"memory_psi_some_avg10_bps\":null}}\n",
        32_u64 * 1024 * 1024 * 1024,
        40_u64 * 1024 * 1024 * 1024,
        9_u64 * 1024 * 1024 * 1024,
    )
    .into_bytes()
}

fn log_frame(record: &[u8]) -> Result<Vec<u8>, String> {
    let length = u32::try_from(record.len()).map_err(|error| error.to_string())?;
    let mut frame = vec![1, 0, 0, 0];
    frame.extend(length.to_be_bytes());
    frame.extend_from_slice(record);
    Ok(frame)
}
