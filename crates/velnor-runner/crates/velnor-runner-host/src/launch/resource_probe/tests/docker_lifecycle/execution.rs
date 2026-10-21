use super::*;
use crate::launch::inspect_tests::{closed, raw_http};
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
enum Failure {
    StartLost,
    WaitLost,
    WaitNonzero,
    LogsLost,
    InvalidRecord,
}

#[tokio::test]
async fn completed_probe_cleans_the_exact_container_before_returning_metrics() -> Result<(), String>
{
    let (scratch, journal, projection) = prepared().await?;
    let cleanup_delay = Duration::from_millis(80);
    let mut responses = start_responses(&projection)?;
    responses.push(http(200, "{}"));
    responses.push(http(200, SELECTED_INFO));
    responses.push(http(200, &inspect(&projection, "running", 0)?));
    responses.push(http(200, r#"{"StatusCode":0,"Error":null}"#));
    responses.push(http(200, &inspect(&projection, "exited", 0)?));
    responses.push(raw_http(200, log_frame(&record_bytes())?));
    append_cleanup(&mut responses, &projection, "exited", 0, cleanup_delay)?;
    let docker = DockerStub::open(responses)?;

    let output = Box::pin(execute::run(
        &docker.docker,
        &journal,
        &projection,
        image_evidence(&projection),
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    ))
    .await;
    let requests = docker.finish_observed().await?;
    let mut expected = start_request_order();
    expected.extend([
        RequestKind::Start,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Wait,
        RequestKind::Inspect,
        RequestKind::Logs,
    ]);
    expected.extend(cleanup_request_order());
    assert_request_order(&requests, &expected)?;
    let output = output.map_err(|error| format!("{error}; requests: {requests:?}"))?;
    let observed_at = output.observed_at;
    assert!(observed_at.elapsed() >= cleanup_delay);

    assert_eq!(requests.len(), 26);
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
async fn parsed_record_with_wrong_running_pid_is_cleaned_without_observation() -> Result<(), String>
{
    let (scratch, journal, projection) = prepared().await?;
    let memory_total = 16 * 1024 * 1024 * 1024;
    let mut record =
        crate::launch::resource_probe::record::ProbeRecord::parse(&record_bytes(), memory_total)
            .ok_or_else(|| "The schema-3 record must parse.".to_owned())?;
    record.probe_host_pid = 4321;
    record.probe_namespace_pid = 4321;
    let mut bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    assert!(
        crate::launch::resource_probe::record::ProbeRecord::parse(&bytes, memory_total).is_some(),
        "The record with matching reported PIDs must parse."
    );

    let mut responses = start_responses(&projection)?;
    responses.push(http(200, "{}"));
    responses.push(http(200, SELECTED_INFO));
    responses.push(http(200, &inspect(&projection, "running", 0)?));
    responses.push(http(200, r#"{"StatusCode":0,"Error":null}"#));
    responses.push(http(200, &inspect(&projection, "exited", 0)?));
    responses.push(raw_http(200, log_frame(&bytes)?));
    append_cleanup(&mut responses, &projection, "exited", 0, Duration::ZERO)?;
    let docker = DockerStub::open(responses)?;

    let result = Box::pin(execute::run(
        &docker.docker,
        &journal,
        &projection,
        image_evidence(&projection),
        memory_total,
        &Deadline::new(),
    ))
    .await;
    let requests = docker.finish_observed().await?;
    let mut expected = start_request_order();
    expected.extend([
        RequestKind::Start,
        RequestKind::Info,
        RequestKind::Inspect,
        RequestKind::Wait,
        RequestKind::Inspect,
        RequestKind::Logs,
    ]);
    expected.extend(cleanup_request_order());
    assert_request_order(&requests, &expected)?;
    assert!(
        matches!(result, Err(HostError::Identity)),
        "The PID check must reject reported PIDs 4321."
    );
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "The rejected sample must complete owned cleanup."
    );
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
        let cleanup_state = append_failure_responses(&mut responses, &projection, failure)?;
        append_cleanup(
            &mut responses,
            &projection,
            cleanup_state,
            9,
            Duration::ZERO,
        )?;
        let docker = DockerStub::open(responses)?;

        assert!(
            Box::pin(execute::run(
                &docker.docker,
                &journal,
                &projection,
                image_evidence(&projection),
                16 * 1024 * 1024 * 1024,
                &Deadline::new(),
            ))
            .await
            .is_err(),
            "failure case {failure:?} unexpectedly yielded metrics"
        );
        let requests = docker.finish_observed().await?;
        assert_request_order(&requests, &failure_request_order(failure))?;
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
    let info = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":8589934592}"#;
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

fn append_failure_responses(
    responses: &mut Vec<DockerResponse>,
    projection: &ProbeProjection,
    failure: Failure,
) -> Result<&'static str, String> {
    if matches!(failure, Failure::StartLost) {
        responses.extend([
            closed(),
            http(200, SELECTED_INFO),
            http(200, &inspect(projection, "exited", 9)?),
        ]);
        return Ok("exited");
    }
    responses.extend([
        http(200, "{}"),
        http(200, SELECTED_INFO),
        http(200, &inspect(projection, "running", 0)?),
    ]);
    match failure {
        Failure::StartLost => unreachable!(),
        Failure::WaitLost => responses.push(closed()),
        Failure::WaitNonzero => responses.push(http(200, r#"{"StatusCode":9,"Error":null}"#)),
        Failure::LogsLost => responses.extend([
            http(200, r#"{"StatusCode":0,"Error":null}"#),
            http(200, &inspect(projection, "exited", 0)?),
            closed(),
        ]),
        Failure::InvalidRecord => responses.extend([
            http(200, r#"{"StatusCode":0,"Error":null}"#),
            http(200, &inspect(projection, "exited", 0)?),
            raw_http(200, log_frame(b"{}\n")?),
        ]),
    }
    Ok("exited")
}

fn failure_request_order(failure: Failure) -> Vec<RequestKind> {
    let mut expected = start_request_order();
    expected.push(RequestKind::Start);
    match failure {
        Failure::StartLost => expected.extend([RequestKind::Info, RequestKind::Inspect]),
        Failure::WaitLost | Failure::WaitNonzero => {
            expected.extend([RequestKind::Info, RequestKind::Inspect, RequestKind::Wait]);
        }
        Failure::LogsLost | Failure::InvalidRecord => expected.extend([
            RequestKind::Info,
            RequestKind::Inspect,
            RequestKind::Wait,
            RequestKind::Inspect,
            RequestKind::Logs,
        ]),
    }
    expected.extend(cleanup_request_order());
    expected
}
