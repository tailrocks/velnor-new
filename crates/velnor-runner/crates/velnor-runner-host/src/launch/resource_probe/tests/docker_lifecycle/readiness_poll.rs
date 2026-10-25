use super::*;
use crate::launch::inspect_tests::raw_http;

use crate::{
    config::{HostConfig, RepositoryConnection},
    config_test_support::{basic_connection, host_config},
    controller_ipc::ControlState,
    launch::resource_probe::provider::{ImageProvider, VerifiedCandidate},
};
use bollard::models::SystemInfo;
use std::future::{Future, ready};

struct ReadinessProvider {
    engine_id: String,
}

impl ImageProvider for ReadinessProvider {
    fn verified_candidate(
        &self,
        _docker: &bollard::Docker,
        _journal: &Journal,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> impl Future<Output = Result<Option<VerifiedCandidate>, HostError>> {
        let candidate: Result<VerifiedCandidate, HostError> = (|| {
            Ok(VerifiedCandidate {
                runtime_id: RUNTIME_ID.to_owned(),
                platform: "linux/amd64".to_owned(),
                daemon_architecture: "x86_64".to_owned(),
                source_revision: REVISION.to_owned(),
                binding_fingerprint: BINDING.to_owned(),
                config: expected_image_config(REVISION),
                engine_id: self.engine_id.clone(),
                docker_root_digest: super::super::super::docker_root_digest(ROOT)?,
                workspace_cleanup_digest: "d".repeat(64),
            })
        })();
        ready(candidate.map(Some))
    }
}

#[tokio::test]
async fn readiness_probe_finishes_while_matching_session_poll_is_pending() -> Result<(), String> {
    let scratch = Scratch::new("readiness-probe-during-poll").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    super::super::configure_probe_capacity(&journal).await?;
    let projection = build_projection(&journal, ENGINE_ID).await?;
    let (control, binding) = pending_control()?;
    let before = control
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The bounded pending poll lost its previous receipt.".to_owned())?;

    let info = format!(
        r#"{{"ID":"{ENGINE_ID}","DockerRootDir":"{ROOT}","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":68719476736}}"#
    );
    let docker = DockerStub::open(pending_poll_responses(&projection, &info)?)?;
    let result = Box::pin(
        super::super::super::readiness::collect_with_provider_and_operation_id(
            &docker.docker,
            &journal,
            &ReadinessProvider {
                engine_id: ENGINE_ID.to_owned(),
            },
            super::OPERATION_ID,
        ),
    )
    .await;
    let requests = docker.finish_observed().await?;
    let observation = result.map_err(|error| {
        format!("The readiness probe failed after requests {requests:?}. Error: {error}.")
    })?;

    assert!(
        observation.is_some(),
        "The readiness probe produced no fresh sample."
    );
    assert_request_order(&requests, &{
        let mut expected = start_request_order();
        expected.splice(0..0, [RequestKind::Info, RequestKind::Info]);
        expected.extend([
            RequestKind::Start,
            RequestKind::Info,
            RequestKind::Inspect,
            RequestKind::Wait,
            RequestKind::Inspect,
            RequestKind::Logs,
        ]);
        expected.extend(cleanup_request_order());
        expected.push(RequestKind::Info);
        expected
    })?;
    let after = control
        .complete_session_poll_receipts()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The probe invalidated the matching pending poll.".to_owned())?;
    assert_eq!(before, after);
    assert_eq!(binding.connection_id.as_str(), before[0].connection_id());
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    crate::launch_harness::absent(&scratch.file())
}

fn pending_poll_responses(
    projection: &ProbeProjection,
    info: &str,
) -> Result<Vec<DockerResponse>, String> {
    let mut responses = start_responses_for_info(projection, info)?;
    responses.splice(0..0, [http(200, info), http(200, info)]);
    responses.extend([
        http(200, "{}"),
        http(200, info),
        http(200, &inspect(projection, "running", 0)?),
        http(200, r#"{"StatusCode":0,"Error":null}"#),
        http(200, &inspect(projection, "exited", 0)?),
        raw_http(200, log_frame(&record_bytes())?),
    ]);
    append_cleanup_for_info(
        &mut responses,
        projection,
        "exited",
        0,
        Duration::ZERO,
        info,
    )?;
    responses.push(http(200, info));
    Ok(responses)
}

async fn build_projection(journal: &Journal, engine_id: &str) -> Result<ProbeProjection, String> {
    let instance_id = journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    ProbeProjection::build(
        super::OPERATION_ID.to_owned(),
        instance_id,
        engine_id.to_owned(),
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
    .map_err(|error| error.to_string())
}

pub(crate) async fn collect_sample_for_engine(
    journal: &Journal,
    engine_id: &str,
) -> Result<crate::launch::ReadinessSample, String> {
    let projection = build_projection(journal, engine_id).await?;
    let info = format!(
        r#"{{"ID":"{engine_id}","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":68719476736}}"#
    );
    let mut responses = super::start_responses_for_info(&projection, &info)?;
    responses.splice(0..0, [http(200, &info), http(200, &info)]);
    responses.extend([
        http(200, "{}"),
        http(200, &info),
        http(200, &super::inspect(&projection, "running", 0)?),
        http(200, r#"{"StatusCode":0,"Error":null}"#),
        http(200, &super::inspect(&projection, "exited", 0)?),
        raw_http(200, super::log_frame(&record_bytes())?),
    ]);
    super::append_cleanup_for_info(
        &mut responses,
        &projection,
        "exited",
        0,
        Duration::ZERO,
        &info,
    )?;
    responses.push(http(200, &info));
    let docker = DockerStub::open(responses)?;
    let result = Box::pin(
        super::super::super::readiness::collect_with_provider_and_operation_id(
            &docker.docker,
            journal,
            &ReadinessProvider {
                engine_id: engine_id.to_owned(),
            },
            super::OPERATION_ID,
        ),
    )
    .await;
    let requests = docker.finish_observed().await?;
    let sample = result
        .map_err(|error| {
            format!("The capacity sample failed after requests {requests:?}. Error: {error}.")
        })?
        .ok_or_else(|| "The controller refresh fixture produced no sample.".to_owned())?;
    Ok(sample)
}

fn pending_control() -> Result<(ControlState, RepositoryConnection), String> {
    let binding_text = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let config_text = host_config(&[binding_text], 1);
    let config = HostConfig::parse(&config_text).map_err(|error| error.to_string())?;
    let binding = config
        .connections()
        .first()
        .cloned()
        .ok_or_else(|| "The test configuration has no repository binding.".to_owned())?;
    let control = ControlState::new(&config);
    control
        .record_session_poll(&binding, 17, 23, "session-1")
        .map_err(|error| error.to_string())?;
    control
        .begin_session_poll(
            &binding,
            17,
            23,
            "session-1",
            std::time::Instant::now() + Duration::from_secs(60),
        )
        .map_err(|error| error.to_string())?;
    Ok((control, binding))
}
