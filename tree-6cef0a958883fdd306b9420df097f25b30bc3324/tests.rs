use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::Instant;

use super::{
    BoundedDockerApi, MAX_HTTP_BUFFER_BYTES, MAX_INVENTORY_RESPONSE_BYTES, acquire_slot,
    decode_slots, decode_with_deadline, receive_decode_result,
};
use crate::HostError;
use crate::worker::inventory::response::BoundedVec;

mod runtime_drop;

fn listener() -> Result<(tempfile::TempDir, String, UnixListener), String> {
    let directory = tempfile::tempdir_in("/tmp").map_err(|error| error.to_string())?;
    let socket = directory.path().join("docker.sock");
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    Ok((directory, socket.to_string_lossy().into_owned(), listener))
}

async fn receive_request(stream: &mut UnixStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before headers".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let request = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
            let mut parts = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace();
            if parts.next() != Some("GET") {
                return Err("unexpected Docker API version or method".to_owned());
            }
            let target = parts
                .next()
                .ok_or_else(|| "missing request target".to_owned())?;
            if !target.starts_with('/') {
                return Err("unexpected Docker API version or method".to_owned());
            }
            return Ok(target.to_owned());
        }
        if bytes.len() > MAX_HTTP_BUFFER_BYTES {
            return Err("request header exceeded fixture limit".to_owned());
        }
    }
}

async fn respond(listener: UnixListener, response: Vec<u8>) -> Result<(), String> {
    let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
    let _target = receive_request(&mut stream).await?;
    stream
        .write_all(&response)
        .await
        .map_err(|error| error.to_string())
}

fn http_response(headers: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 200 OK\r\n{headers}\r\n").into_bytes();
    response.extend_from_slice(body);
    response
}

fn json_response(body: &[u8]) -> Vec<u8> {
    http_response(
        &format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ),
        body,
    )
}

async fn serve_inventory(
    listener: UnixListener,
    fail_volumes: bool,
) -> Result<Vec<String>, String> {
    let mut routes = Vec::with_capacity(3);
    for (index, expected) in [
        "/v1.53/containers/json?all=1",
        "/v1.53/networks",
        "/v1.53/volumes",
    ]
    .into_iter()
    .enumerate()
    {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let route = receive_request(&mut stream).await?;
        if route != expected {
            return Err(format!("unexpected route {route}"));
        }
        routes.push(route);
        let response = if fail_volumes && index == 2 {
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_vec()
        } else if index < 2 {
            json_response(b"[]")
        } else {
            json_response(br#"{"Volumes":[],"Warnings":[]}"#)
        };
        stream
            .write_all(&response)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(routes)
}

#[tokio::test]
async fn bounded_http_client_fetches_and_decodes_one_docker_json_response() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(respond(
        listener,
        http_response(
            "Content-Type: application/json\r\nContent-Length: 5\r\n",
            b"[1,2]",
        ),
    ));
    let api = BoundedDockerApi::new(&socket).map_err(|error| format!("client: {error}"))?;
    let decoded: BoundedVec<u8, 8> = api
        .get_json(
            "/containers/json?all=1",
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .map_err(|error| format!("decode: {error}"))?;
    assert_eq!(decoded.into_vec(), [1, 2]);
    server.await.map_err(|error| error.to_string())??;
    Ok(())
}

#[tokio::test]
async fn declared_oversized_body_is_rejected_before_body_arrives() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        MAX_INVENTORY_RESPONSE_BYTES + 1
    )
    .into_bytes();
    let server = tokio::spawn(respond(listener, response));
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let result: Result<BoundedVec<u8, 8>, HostError> = api
        .get_json(
            "/containers/json?all=1",
            Instant::now() + Duration::from_secs(2),
        )
        .await;
    assert_eq!(result, Err(HostError::Docker));
    server.await.map_err(|error| error.to_string())??;
    Ok(())
}

#[tokio::test]
async fn chunked_body_over_test_limit_is_rejected_while_streaming() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(respond(
        listener,
        http_response(
            "Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n",
            b"5\r\n[1,2]\r\n0\r\n\r\n",
        ),
    ));
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let result: Result<BoundedVec<u8, 8>, HostError> = api
        .get_json_with_limit(
            "/containers/json?all=1",
            Instant::now() + Duration::from_secs(2),
            4,
        )
        .await;
    assert_eq!(result, Err(HostError::Docker));
    server.await.map_err(|error| error.to_string())??;
    Ok(())
}

#[tokio::test]
async fn silent_body_expires_at_the_callers_absolute_deadline() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let _target = receive_request(&mut stream).await?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10\r\n\r\n",
            )
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok::<_, String>(())
    });
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(50);
    let result: Result<BoundedVec<u8, 8>, HostError> =
        api.get_json("/containers/json?all=1", deadline).await;
    assert_eq!(result, Err(HostError::Docker));
    server.abort();
    Ok(())
}

#[tokio::test]
async fn trickling_body_does_not_reset_the_absolute_deadline() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let _target = receive_request(&mut stream).await?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .await
            .map_err(|error| error.to_string())?;
        for _ in 0..20 {
            if stream.write_all(b"1\r\nx\r\n").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        Ok::<_, String>(())
    });
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(80);
    let result: Result<BoundedVec<u8, 8>, HostError> =
        api.get_json("/containers/json?all=1", deadline).await;
    assert_eq!(result, Err(HostError::Docker));
    server.abort();
    Ok(())
}

#[tokio::test]
async fn oversized_response_headers_fail_closed() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let mut headers = "Content-Type: application/json\r\nX-Large: ".to_owned();
    headers.push_str(&"x".repeat(MAX_HTTP_BUFFER_BYTES * 2));
    headers.push_str("\r\nContent-Length: 2\r\n");
    let server = tokio::spawn(respond(listener, http_response(&headers, b"[]")));
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let result: Result<BoundedVec<u8, 8>, HostError> = api
        .get_json(
            "/containers/json?all=1",
            Instant::now() + Duration::from_secs(2),
        )
        .await;
    assert_eq!(result, Err(HostError::Docker));
    server.abort();
    Ok(())
}

#[tokio::test]
async fn malformed_json_is_not_returned_as_an_empty_inventory() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(respond(
        listener,
        http_response(
            "Content-Type: application/json\r\nContent-Length: 5\r\n",
            b"nope!",
        ),
    ));
    let api = BoundedDockerApi::new(&socket).map_err(|error| error.to_string())?;
    let result: Result<BoundedVec<u8, 8>, HostError> = api
        .get_json(
            "/containers/json?all=1",
            Instant::now() + Duration::from_secs(2),
        )
        .await;
    assert_eq!(result, Err(HostError::Docker));
    server.await.map_err(|error| error.to_string())??;
    Ok(())
}

#[tokio::test]
async fn expired_decode_never_returns_a_late_success() -> Result<(), String> {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let permit = acquire_slot(decode_slots(), Instant::now() + Duration::from_secs(1))
        .await
        .map_err(|error| error.to_string())?;
    let task = tokio::spawn(decode_with_deadline(
        Vec::new(),
        Instant::now() + Duration::from_millis(50),
        permit,
        move |_| {
            started_tx.send(()).expect("test receiver remains alive");
            release_rx.recv().map_err(|_| HostError::Docker)?;
            Ok::<_, HostError>("late decode")
        },
    ));
    started_rx.await.map_err(|error| error.to_string())?;
    assert_eq!(
        task.await.map_err(|error| error.to_string())?,
        Err(HostError::Docker)
    );
    release_tx.send(()).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn complete_inventory_uses_each_unfiltered_endpoint_in_order() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(serve_inventory(listener, false));
    let inventory = crate::worker::list_owned_docker_resources_until(
        &socket,
        Instant::now() + Duration::from_secs(2),
    )
    .await
    .map_err(|error| format!("inventory: {error}"))?;
    assert_eq!(inventory.len(), 0);
    let routes = server.await.map_err(|error| error.to_string())??;
    assert_eq!(
        routes,
        [
            "/v1.53/containers/json?all=1",
            "/v1.53/networks",
            "/v1.53/volumes"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn late_endpoint_failure_never_returns_a_partial_inventory() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(serve_inventory(listener, true));
    let inventory = crate::worker::list_owned_docker_resources_until(
        &socket,
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    assert_eq!(inventory, Err(HostError::Docker));
    let routes = server.await.map_err(|error| error.to_string())??;
    assert_eq!(routes.len(), 3);
    Ok(())
}
