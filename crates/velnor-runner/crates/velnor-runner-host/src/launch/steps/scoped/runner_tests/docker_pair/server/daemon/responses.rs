use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub(super) async fn record_jit_input(
    stream: &mut UnixStream,
    jit_input: &std::sync::Mutex<Vec<u8>>,
) -> Result<(), String> {
    stream
        .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
        .await
        .map_err(|error| error.to_string())?;
    let mut payload = Vec::new();
    stream
        .read_to_end(&mut payload)
        .await
        .map_err(|error| error.to_string())?;
    jit_input
        .lock()
        .map_err(|_| "The JIT configuration lock failed.".to_owned())?
        .extend(payload);
    Ok(())
}

pub(super) async fn write_response(
    stream: &mut UnixStream,
    status: u16,
    body: &[u8],
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
