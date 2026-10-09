use std::time::Duration;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::time::Instant;

use crate::{connect_unix_bound, observe_docker_daemon_binding_until};

use super::{FixtureServer, accept_bounded, read_request, respond_info, socket_path};

fn exec_upgrade_server(listener: UnixListener) -> FixtureServer<Result<[String; 7], String>> {
    FixtureServer::spawn(async move {
        let (mut initial, _) = accept_bounded(&listener).await?;
        let observed = read_request(&mut initial)
            .await?
            .ok_or("missing initial identity request")?;
        if observed != "GET /info HTTP/1.1" {
            return Err(format!("unexpected initial identity request: {observed}"));
        }
        respond_info(&mut initial, "engine-A").await?;
        drop(initial);

        let (mut create_stream, _) = accept_bounded(&listener).await?;
        let create_info = read_request(&mut create_stream)
            .await?
            .ok_or("missing create identity request")?;
        respond_info(&mut create_stream, "engine-A").await?;
        let create = read_request(&mut create_stream)
            .await?
            .ok_or("missing create exec request")?;
        if create != "POST /containers/container-id/exec HTTP/1.1" {
            return Err(format!("unexpected create exec request: {create}"));
        }
        create_stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"Id\":\"exec-id\"}")
            .await
            .map_err(|error| error.to_string())?;
        drop(create_stream);

        let (mut start_stream, _) = accept_bounded(&listener).await?;
        let start_info = read_request(&mut start_stream)
            .await?
            .ok_or("missing start identity request")?;
        respond_info(&mut start_stream, "engine-A").await?;
        let start = read_request(&mut start_stream)
            .await?
            .ok_or("missing start exec request")?;
        if start != "POST /exec/exec-id/start HTTP/1.1" {
            return Err(format!("unexpected start exec request: {start}"));
        }
        start_stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n",
            )
            .await
            .map_err(|error| error.to_string())?;
        start_stream
            .write_all(&[1, 0, 0, 0, 0, 0, 0, 3, b'o', b'k', b'\n'])
            .await
            .map_err(|error| error.to_string())?;
        drop(start_stream);

        let (mut inspect_stream, _) = accept_bounded(&listener).await?;
        let inspect_info = read_request(&mut inspect_stream)
            .await?
            .ok_or("missing inspect identity request")?;
        respond_info(&mut inspect_stream, "engine-A").await?;
        let inspect = read_request(&mut inspect_stream)
            .await?
            .ok_or("missing inspect exec request")?;
        if inspect != "GET /exec/exec-id/json HTTP/1.1" {
            return Err(format!("unexpected inspect exec request: {inspect}"));
        }
        inspect_stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 45\r\n\r\n{\"ID\":\"exec-id\",\"Running\":false,\"ExitCode\":0}")
            .await
            .map_err(|error| error.to_string())?;
        Ok([
            observed,
            create_info,
            create,
            start_info,
            start,
            inspect_info,
            inspect,
        ])
    })
}

#[tokio::test]
async fn bollard_exec_upgrade_keeps_guarded_connection_driver_alive() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let server = exec_upgrade_server(listener);

    let deadline = Instant::now() + Duration::from_secs(3);
    let binding = observe_docker_daemon_binding_until(&endpoint, deadline)
        .await
        .map_err(|_| "initial identity observation failed".to_owned())?;
    let docker = connect_unix_bound(&binding).map_err(|_| "bound client failed".to_owned())?;
    let created = docker
        .create_exec(
            "container-id",
            CreateExecOptions::<String> {
                attach_stdout: Some(true),
                cmd: Some(vec!["/bin/true".to_owned()]),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("create exec failed: {error:?}"))?;
    assert_eq!(created.id, "exec-id");

    let start = docker
        .start_exec(&created.id, Some(StartExecOptions::default()))
        .await
        .map_err(|error| format!("start exec failed: {error:?}"))?;
    let StartExecResults::Attached { mut output, input } = start else {
        return Err("exec did not attach".to_owned());
    };
    drop(input);
    let line = tokio::time::timeout(Duration::from_secs(1), output.next())
        .await
        .map_err(|_| "upgraded output timed out".to_owned())?
        .ok_or("upgraded output ended before frame")?
        .map_err(|error| format!("upgraded output failed: {error:?}"))?;
    match line {
        LogOutput::StdOut { message } if message.as_ref() == b"ok\n" => {}
        other => return Err(format!("unexpected exec output {other:?}")),
    }

    let inspected = docker
        .inspect_exec(&created.id)
        .await
        .map_err(|error| format!("inspect exec failed: {error:?}"))?;
    assert_eq!(inspected.id.as_deref(), Some("exec-id"));
    assert_eq!(inspected.running, Some(false));
    let events = server.finish().await??;
    assert_eq!(
        events,
        [
            "GET /info HTTP/1.1",
            "GET /info HTTP/1.1",
            "POST /containers/container-id/exec HTTP/1.1",
            "GET /info HTTP/1.1",
            "POST /exec/exec-id/start HTTP/1.1",
            "GET /info HTTP/1.1",
            "GET /exec/exec-id/json HTTP/1.1"
        ]
    );
    Ok(())
}
