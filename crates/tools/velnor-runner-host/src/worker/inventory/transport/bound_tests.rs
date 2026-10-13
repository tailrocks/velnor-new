use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::Instant;

use crate::HostError;

fn listener() -> Result<(tempfile::TempDir, String, UnixListener), String> {
    let directory = tempfile::tempdir_in("/tmp").map_err(|error| error.to_string())?;
    let path = directory.path().join("docker.sock");
    let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    Ok((directory, path.to_string_lossy().into_owned(), listener))
}

async fn request_line(stream: &mut UnixStream) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        let count = stream
            .read(&mut byte)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("connection ended in request headers".to_owned())
            };
        }
        bytes.push(byte[0]);
        if bytes.len() > 16 * 1024 {
            return Err("request headers exceeded fixture limit".to_owned());
        }
    }
    let header = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    let line = header.lines().next().ok_or("missing request line")?;
    let mut fields = line.split_whitespace();
    if fields.next() != Some("GET") {
        return Err("unexpected method in inventory fixture".to_owned());
    }
    Ok(fields.next().map(str::to_owned))
}

async fn write_json(stream: &mut UnixStream, body: &[u8]) -> Result<(), String> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())
}

async fn serve_bound_inventory(
    listener: UnixListener,
    mismatch_at: Option<usize>,
) -> Result<Vec<String>, String> {
    let mut events = Vec::with_capacity(7);
    let (mut initial, _) = listener.accept().await.map_err(|error| error.to_string())?;
    let route = request_line(&mut initial)
        .await?
        .ok_or("missing initial info")?;
    if route != "/info" {
        return Err(format!("unexpected initial observation route {route}"));
    }
    events.push(route);
    write_json(&mut initial, br#"{"ID":"engine-A"}"#).await?;
    drop(initial);

    for (index, expected) in [
        "/v1.53/containers/json?all=1",
        "/v1.53/networks",
        "/v1.53/volumes",
    ]
    .into_iter()
    .enumerate()
    {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let info = request_line(&mut stream)
            .await?
            .ok_or("missing same-connection info")?;
        if info != "/v1.53/info" {
            return Err(format!("unexpected daemon identity route {info}"));
        }
        events.push(info);
        let engine_id = if mismatch_at == Some(index) {
            "engine-B"
        } else {
            "engine-A"
        };
        let info_body = serde_json::json!({"ID": engine_id}).to_string();
        write_json(&mut stream, info_body.as_bytes()).await?;
        if mismatch_at == Some(index) {
            let next =
                tokio::time::timeout(Duration::from_millis(200), request_line(&mut stream)).await;
            if let Ok(Ok(Some(route))) = next {
                events.push(route);
            }
            return Ok(events);
        }

        let route = request_line(&mut stream)
            .await?
            .ok_or("missing list route")?;
        if route != expected {
            return Err(format!("unexpected inventory route {route}"));
        }
        events.push(route);
        let response = if index < 2 {
            b"[]".as_slice()
        } else {
            br#"{"Volumes":[],"Warnings":[]}"#.as_slice()
        };
        write_json(&mut stream, response).await?;
    }
    Ok(events)
}

#[tokio::test]
async fn bound_inventory_checks_identity_on_each_list_connection() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(serve_bound_inventory(listener, None));
    let deadline = Instant::now() + Duration::from_secs(2);
    let binding = crate::observe_docker_daemon_binding_until(&socket, deadline)
        .await
        .map_err(|_| "initial identity observation failed".to_owned())?;
    let inventory = crate::worker::list_owned_docker_resources_bound_until(&binding, deadline)
        .await
        .map_err(|_| "bound inventory failed".to_owned())?;
    assert_eq!(
        inventory,
        Vec::<crate::worker::inventory::OwnedDockerResource>::new()
    );
    assert_eq!(
        server.await.map_err(|error| error.to_string())??,
        [
            "/info",
            "/v1.53/info",
            "/v1.53/containers/json?all=1",
            "/v1.53/info",
            "/v1.53/networks",
            "/v1.53/info",
            "/v1.53/volumes"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn bound_inventory_mismatch_never_returns_a_partial_snapshot() -> Result<(), String> {
    let (_directory, socket, listener) = listener()?;
    let server = tokio::spawn(serve_bound_inventory(listener, Some(1)));
    let deadline = Instant::now() + Duration::from_secs(2);
    let binding = crate::observe_docker_daemon_binding_until(&socket, deadline)
        .await
        .map_err(|_| "initial identity observation failed".to_owned())?;
    let inventory =
        crate::worker::list_owned_docker_resources_bound_until(&binding, deadline).await;
    assert_eq!(inventory, Err(HostError::Docker));
    assert_eq!(
        server.await.map_err(|error| error.to_string())??,
        [
            "/info",
            "/v1.53/info",
            "/v1.53/containers/json?all=1",
            "/v1.53/info"
        ]
    );
    Ok(())
}
