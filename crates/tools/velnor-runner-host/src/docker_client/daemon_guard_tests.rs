use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::Instant;

use crate::{connect_unix_bound, observe_docker_daemon_binding_until};
use http_body_util::Empty;
use hyper::body::Bytes;
use hyper::{Method, Request};

#[path = "daemon_guard_fixture.rs"]
mod fixture;
use fixture::{FixtureServer, accept_bounded};

static NEXT_SOCKET: AtomicUsize = AtomicUsize::new(0);

fn socket_path() -> Result<(tempfile::TempDir, String, UnixListener), String> {
    let directory = tempfile::tempdir_in("/tmp").map_err(|error| error.to_string())?;
    let path = directory.path().join(format!(
        "docker-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    Ok((directory, path.to_string_lossy().into_owned(), listener))
}

async fn read_request(stream: &mut UnixStream) -> Result<Option<String>, String> {
    fixture::bounded(
        fixture::STEP_TIMEOUT,
        read_request_inner(stream),
        "request read",
    )
    .await
}

async fn read_request_inner(stream: &mut UnixStream) -> Result<Option<String>, String> {
    const MAX_HEADERS: usize = 16 * 1024;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("connection closed inside request headers".to_owned())
            };
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > MAX_HEADERS {
            return Err("request headers exceeded fixture bound".to_owned());
        }
    };
    let (request_line, content_length) = {
        let header_text =
            std::str::from_utf8(&bytes[..header_end]).map_err(|error| error.to_string())?;
        let content_length = header_text
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        (
            header_text.lines().next().map(str::to_owned),
            content_length,
        )
    };
    if content_length > 64 * 1024 {
        return Err("request body exceeded fixture bound".to_owned());
    }
    let total = header_end + content_length;
    while bytes.len() < total {
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("connection closed inside request body".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(request_line)
}

async fn respond(stream: &mut UnixStream, status: &str, body: &[u8]) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())
}

async fn respond_info(stream: &mut UnixStream, engine_id: &str) -> Result<(), String> {
    let body = serde_json::json!({"ID": engine_id}).to_string();
    respond(stream, "200 OK", body.as_bytes()).await
}

#[tokio::test]
async fn bound_bollard_request_checks_info_and_dispatches_once_on_same_socket() -> Result<(), String>
{
    let (_directory, endpoint, listener) = socket_path()?;
    let server = FixtureServer::spawn(async move {
        let (mut observed, _) = accept_bounded(&listener).await?;
        let request = read_request(&mut observed)
            .await?
            .ok_or("missing observation")?;
        if request != "GET /info HTTP/1.1" {
            return Err(format!("unexpected observation request: {request}"));
        }
        respond_info(&mut observed, "engine-A").await?;
        drop(observed);

        let (mut bound, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut bound)
            .await?
            .ok_or("missing bound info")?;
        if info != "GET /info HTTP/1.1" {
            return Err(format!("unexpected bound info: {info}"));
        }
        respond_info(&mut bound, "engine-A").await?;
        let ping = read_request(&mut bound).await?.ok_or("missing ping")?;
        if ping != "GET /_ping HTTP/1.1" {
            return Err(format!("unexpected original request: {ping}"));
        }
        bound
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>([request, info, ping])
    });

    let binding =
        observe_docker_daemon_binding_until(&endpoint, Instant::now() + Duration::from_secs(2))
            .await
            .map_err(|_| "initial observation failed".to_owned())?;
    assert_eq!(binding.endpoint(), endpoint);
    assert_eq!(binding.engine_id(), "engine-A");
    assert_eq!(
        format!("{binding:?}"),
        "DockerDaemonBinding { endpoint: \"<redacted>\", engine_id: \"<redacted>\" }"
    );
    let docker = connect_unix_bound(&binding).map_err(|_| "bound client failed".to_owned())?;
    let ping = docker.ping().await;
    let events = server.finish().await??;
    ping.map_err(|error| format!("same-connection ping failed: {error:?}"))?;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], "GET /info HTTP/1.1");
    assert_eq!(events[1], "GET /info HTTP/1.1");
    assert_eq!(events[2], "GET /_ping HTTP/1.1");
    Ok(())
}

#[tokio::test]
async fn daemon_id_mismatch_never_dispatches_the_original_request() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let server = FixtureServer::spawn(async move {
        let (mut initial, _) = accept_bounded(&listener).await?;
        let _ = read_request(&mut initial)
            .await?
            .ok_or("missing initial info")?;
        respond_info(&mut initial, "engine-A").await?;
        drop(initial);

        let (mut replacement, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut replacement)
            .await?
            .ok_or("missing checked info")?;
        respond_info(&mut replacement, "engine-B").await?;
        let original =
            tokio::time::timeout(Duration::from_millis(200), read_request(&mut replacement)).await;
        let dispatched = match original {
            Ok(Ok(Some(request))) => Some(request),
            Ok(Err(error)) => return Err(error),
            Ok(Ok(None)) | Err(_) => None,
        };
        Ok::<_, String>((info, dispatched))
    });

    let binding =
        observe_docker_daemon_binding_until(&endpoint, Instant::now() + Duration::from_secs(2))
            .await
            .map_err(|_| "initial observation failed".to_owned())?;
    let docker = connect_unix_bound(&binding).map_err(|_| "bound client failed".to_owned())?;
    assert!(docker.ping().await.is_err());
    let (info, dispatched) = server.finish().await??;
    assert_eq!(info, "GET /info HTTP/1.1");
    assert_eq!(dispatched, None);
    Ok(())
}

#[tokio::test]
async fn socket_replacement_after_info_cannot_receive_original_request() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let replacement_path = endpoint.clone();
    let server = FixtureServer::spawn(async move {
        let (mut initial, _) = accept_bounded(&listener).await?;
        let _ = read_request(&mut initial)
            .await?
            .ok_or("missing initial info")?;
        respond_info(&mut initial, "engine-A").await?;
        drop(initial);

        let (mut old_daemon, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut old_daemon)
            .await?
            .ok_or("missing bound info")?;
        if info != "GET /info HTTP/1.1" {
            return Err(format!("unexpected bound info: {info}"));
        }
        std::fs::remove_file(&replacement_path).map_err(|error| error.to_string())?;
        let replacement =
            UnixListener::bind(&replacement_path).map_err(|error| error.to_string())?;
        respond_info(&mut old_daemon, "engine-A").await?;
        drop(old_daemon);

        let accepted = tokio::time::timeout(Duration::from_millis(250), replacement.accept()).await;
        match accepted {
            Ok(Ok((mut stream, _))) => read_request(&mut stream).await,
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Ok(None),
        }
    });

    let binding =
        observe_docker_daemon_binding_until(&endpoint, Instant::now() + Duration::from_secs(2))
            .await
            .map_err(|_| "initial observation failed".to_owned())?;
    let docker = connect_unix_bound(&binding).map_err(|_| "bound client failed".to_owned())?;
    assert!(docker.ping().await.is_err());
    assert_eq!(server.finish().await??, None);
    Ok(())
}

#[tokio::test]
async fn deadline_while_info_is_pending_closes_connection_without_original_request()
-> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = FixtureServer::spawn(async move {
        let (mut stream, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut stream).await?.ok_or("missing info")?;
        seen_tx.send(()).map_err(|()| "test receiver dropped")?;
        let next = read_request(&mut stream).await?;
        Ok::<_, String>((info, next))
    });
    let request = Request::builder()
        .method(Method::GET)
        .uri("http://localhost/v1.53/_ping")
        .body(Empty::<Bytes>::new())
        .map_err(|error| error.to_string())?;
    let call = tokio::spawn(async move {
        super::guarded_request(
            &endpoint,
            "engine-A",
            request,
            Empty::<Bytes>::new,
            Instant::now() + Duration::from_millis(80),
        )
        .await
    });
    seen_rx.await.map_err(|error| error.to_string())?;
    assert!(call.await.map_err(|error| error.to_string())?.is_err());
    let (info, next) = server.finish().await??;
    assert_eq!(info, "GET /v1.53/info HTTP/1.1");
    assert_eq!(next, None);
    Ok(())
}

#[tokio::test]
async fn caller_cancellation_during_info_never_dispatches_original_request() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let (continue_tx, continue_rx) = tokio::sync::oneshot::channel();
    let server = FixtureServer::spawn(async move {
        let (mut observed, _) = accept_bounded(&listener).await?;
        let _ = read_request(&mut observed)
            .await?
            .ok_or("missing initial info")?;
        respond_info(&mut observed, "engine-A").await?;
        drop(observed);
        let (mut stream, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut stream)
            .await?
            .ok_or("missing guarded info")?;
        seen_tx.send(()).map_err(|()| "test receiver dropped")?;
        drop(continue_rx.await);
        drop(respond_info(&mut stream, "engine-A").await);
        let next = read_request(&mut stream).await?;
        Ok::<_, String>((info, next))
    });

    let binding =
        observe_docker_daemon_binding_until(&endpoint, Instant::now() + Duration::from_secs(2))
            .await
            .map_err(|_| "initial observation failed".to_owned())?;
    let docker = connect_unix_bound(&binding).map_err(|_| "bound client failed".to_owned())?;
    let request = tokio::spawn(async move { docker.ping().await });
    seen_rx.await.map_err(|error| error.to_string())?;
    request.abort();
    drop(request.await);
    continue_tx
        .send(())
        .map_err(|()| "server continuation dropped")?;
    let (info, next) = server.finish().await??;
    assert_eq!(info, "GET /info HTTP/1.1");
    assert_eq!(next, None);
    Ok(())
}

#[tokio::test]
async fn unknown_length_info_body_is_capped_while_streaming() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let server = FixtureServer::spawn(async move {
        let (mut stream, _) = accept_bounded(&listener).await?;
        let route = read_request(&mut stream)
            .await?
            .ok_or("missing info request")?;
        let body = vec![b'x'; super::MAX_DAEMON_INFO_BYTES + 1];
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            body.len()
        );
        stream
            .write_all(header.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        drop(stream.write_all(&body).await);
        drop(stream.write_all(b"\r\n0\r\n\r\n").await);
        Ok::<_, String>(route)
    });
    let result =
        observe_docker_daemon_binding_until(&endpoint, Instant::now() + Duration::from_secs(2))
            .await;
    assert!(result.is_err());
    assert_eq!(server.finish().await??, "GET /info HTTP/1.1");
    Ok(())
}

#[path = "daemon_guard_upgrade_tests.rs"]
mod upgrade_tests;

#[path = "daemon_guard_deadline_tests.rs"]
mod deadline_tests;
