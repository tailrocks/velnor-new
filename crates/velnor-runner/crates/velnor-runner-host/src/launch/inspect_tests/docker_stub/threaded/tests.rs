use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::super::{DockerResponse, request::RequestTrace};
use super::{BackgroundDockerStub, state::ServerState};

#[test]
fn dropping_without_a_request_stops_the_server_quickly() -> Result<(), String> {
    drop_within_timeout(BackgroundDockerStub::open(Vec::new())?, None)
}

#[test]
fn dropping_during_a_partial_request_unblocks_the_server() -> Result<(), String> {
    let stub = BackgroundDockerStub::open(Vec::new())?;
    let mut client = UnixStream::connect(&stub.path).map_err(|error| error.to_string())?;
    client
        .write_all(b"GET /containers/partial HTTP/1.1\r\nHost: local\r\n")
        .map_err(|error| error.to_string())?;

    if let Err(error) = wait_for_active_stream(&stub.state) {
        let _cleanup = drop_within_timeout(stub, Some(client));
        return Err(error);
    }

    drop_within_timeout(stub, Some(client))
}

#[tokio::test]
async fn a_complete_request_keeps_its_response_trace() -> Result<(), String> {
    let stub = BackgroundDockerStub::open(vec![DockerResponse {
        status: Some(200),
        body: b"{}".to_vec(),
        delay: Duration::ZERO,
    }])?;
    let observed = Arc::clone(&stub.observed);
    let mut client = UnixStream::connect(&stub.path).map_err(|error| error.to_string())?;
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    client
        .write_all(
            b"GET /containers/runner/json?size=false HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
        )
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    drop(client);

    let requests = stub.finish_observed().await?;
    if requests != ["GET /containers/runner/json?size=false HTTP/1.1"] {
        return Err(format!(
            "The server recorded the wrong request line: {requests:?}."
        ));
    }
    let traces = request_traces(&observed)?;
    if traces
        != [RequestTrace {
            path: Some("/containers/runner/json".to_owned()),
            response_index: Some(0),
            response_status: Some(200),
            failure: None,
        }]
    {
        return Err(format!(
            "The server changed the valid request trace: {traces:?}."
        ));
    }
    Ok(())
}

#[tokio::test]
async fn explicit_finish_preserves_response_write_failures() -> Result<(), String> {
    let stub = BackgroundDockerStub::open(vec![DockerResponse {
        status: Some(200),
        body: b"{}".to_vec(),
        delay: Duration::from_millis(50),
    }])?;
    let observed = Arc::clone(&stub.observed);
    let mut client = UnixStream::connect(&stub.path).map_err(|error| error.to_string())?;
    client
        .write_all(
            b"GET /containers/runner/json HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n",
        )
        .map_err(|error| error.to_string())?;
    drop(client);
    wait_for_observed_request(&observed)?;

    if stub.finish_observed().await.is_ok() {
        return Err("explicit finish ignored a failed response write".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn a_malformed_request_keeps_its_failure_trace() -> Result<(), String> {
    let stub = BackgroundDockerStub::open(Vec::new())?;
    let observed = Arc::clone(&stub.observed);
    let mut client = UnixStream::connect(&stub.path).map_err(|error| error.to_string())?;
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    client
        .write_all(b"GET /containers/runner/json HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    drop(client);

    if stub.finish_observed().await.is_ok() {
        return Err("The server accepted a malformed request.".to_owned());
    }
    let traces = request_traces(&observed)?;
    let Some(trace) = traces.first() else {
        return Err("The server omitted the malformed request trace.".to_owned());
    };
    if traces.len() != 1
        || trace.path.as_deref() != Some("/containers/runner/json")
        || trace.response_index.is_some()
        || trace.response_status.is_some()
        || trace.failure.as_deref().is_none_or(str::is_empty)
    {
        return Err(format!(
            "The server changed the malformed request trace: {traces:?}."
        ));
    }
    Ok(())
}

fn wait_for_active_stream(state: &ServerState) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if state.has_active_stream() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err("The server did not register the partial request.".to_owned())
}

fn wait_for_observed_request(
    observed: &Arc<std::sync::Mutex<Vec<RequestTrace>>>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !observed
            .lock()
            .map_err(|_| "The test could not lock the Docker request trace.".to_owned())?
            .is_empty()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err("The server did not observe the complete request.".to_owned())
}

fn request_traces(
    observed: &Arc<std::sync::Mutex<Vec<RequestTrace>>>,
) -> Result<Vec<RequestTrace>, String> {
    observed
        .lock()
        .map(|traces| traces.clone())
        .map_err(|_| "The test could not lock the Docker request trace.".to_owned())
}

fn drop_within_timeout(
    stub: BackgroundDockerStub,
    mut client: Option<UnixStream>,
) -> Result<(), String> {
    let state = Arc::clone(&stub.state);
    let (completed, finished) = mpsc::channel();
    let dropper = thread::Builder::new()
        .name("velnor-docker-test-drop".to_owned())
        .spawn(move || {
            drop(stub);
            let _sent = completed.send(());
        })
        .map_err(|error| error.to_string())?;

    match finished.recv_timeout(Duration::from_secs(2)) {
        Ok(()) => {
            dropper
                .join()
                .map_err(|_| "The Docker stub drop thread failed.".to_owned())?;
            drop(client);
            Ok(())
        }
        Err(error) => {
            state.request_stop();
            drop(client.take());
            if finished.recv_timeout(Duration::from_secs(2)).is_ok() {
                let _joined = dropper.join();
            }
            Err(format!(
                "Dropping the Docker stub exceeded two seconds: {error}."
            ))
        }
    }
}
