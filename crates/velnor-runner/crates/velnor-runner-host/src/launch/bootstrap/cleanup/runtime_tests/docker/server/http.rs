use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::error::HostError;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: Vec<u8>,
}

pub(super) async fn read_request(stream: &mut UnixStream) -> Result<Request, HostError> {
    let mut bytes = Vec::new();
    let mut buf = [0_u8; 4096];
    let header_end;
    loop {
        let count = stream.read(&mut buf).await.map_err(|_| HostError::Docker)?;
        if count == 0 || bytes.len() > 128 * 1024 {
            return Err(HostError::Frame);
        }
        bytes.extend_from_slice(&buf[..count]);
        if let Some(index) = bytes.windows(4).position(|item| item == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| HostError::Frame)?;
    let mut lines = headers.split("\r\n");
    let mut first = lines.next().ok_or(HostError::Frame)?.split_whitespace();
    let method = first.next().ok_or(HostError::Frame)?.to_owned();
    let path = first.next().ok_or(HostError::Frame)?.to_owned();
    let length = lines
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = stream.read(&mut buf).await.map_err(|_| HostError::Docker)?;
        if count == 0 {
            return Err(HostError::Frame);
        }
        bytes.extend_from_slice(&buf[..count]);
    }
    Ok(Request {
        method,
        path,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

pub(super) async fn read_line(stream: &mut UnixStream) -> Result<Vec<u8>, HostError> {
    let mut output = Vec::new();
    loop {
        let byte = stream.read_u8().await.map_err(|_| HostError::Docker)?;
        output.push(byte);
        if byte == b'\n' {
            return Ok(output);
        }
        if output.len() > 32 * 1024 {
            return Err(HostError::Frame);
        }
    }
}

pub(super) async fn write_response(
    stream: &mut UnixStream,
    status: u16,
    body: &[u8],
) -> Result<(), HostError> {
    let phrase = if status == 200 {
        "OK"
    } else if status == 201 {
        "Created"
    } else if status == 204 {
        "No Content"
    } else {
        "Not Found"
    };
    let header = format!(
        "HTTP/1.1 {status} {phrase}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|_| HostError::Docker)?;
    stream.write_all(body).await.map_err(|_| HostError::Docker)
}
