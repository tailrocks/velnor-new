use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use super::MAX_CAPTURE_BYTES;

const MAX_HEADER_BYTES: usize = 64 * 1024;

pub(super) struct RequestHead {
    pub(super) line: String,
    pub(super) transfer_chunked: bool,
    pub(super) content_length: usize,
}

pub(super) async fn read_head(stream: &mut UnixStream) -> io::Result<RequestHead> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized HTTP headers",
            ));
        }
        if stream.read(&mut byte).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short HTTP headers",
            ));
        }
        bytes.push(byte[0]);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = text.lines();
    let line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty HTTP request"))?
        .to_owned();
    let mut transfer_chunked = false;
    let mut content_length = 0;
    for header in lines.filter_map(|line| line.split_once(':')) {
        if header.0.eq_ignore_ascii_case("transfer-encoding") {
            transfer_chunked = header.1.to_ascii_lowercase().contains("chunked");
        } else if header.0.eq_ignore_ascii_case("content-length") {
            content_length = header
                .1
                .trim()
                .parse()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
    }
    Ok(RequestHead {
        line,
        transfer_chunked,
        content_length,
    })
}

pub(super) async fn read_request_body(
    stream: &mut UnixStream,
    head: &RequestHead,
    body: &mut Vec<u8>,
) -> io::Result<bool> {
    if head.transfer_chunked {
        read_chunked_body(stream, body).await
    } else {
        read_exact_body(stream, body, head.content_length).await
    }
}

pub(super) async fn read_first_chunk(
    stream: &mut UnixStream,
    head: &RequestHead,
    body: &mut Vec<u8>,
) -> io::Result<bool> {
    if !head.transfer_chunked {
        return read_exact_body(stream, body, head.content_length.min(64 * 1024)).await;
    }
    let line = read_crlf_line(stream).await?;
    let size_text = std::str::from_utf8(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default(), 16)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if size == 0 || size > MAX_CAPTURE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing upload chunk",
        ));
    }
    read_exact_body(stream, body, size).await
}

async fn read_chunked_body(stream: &mut UnixStream, body: &mut Vec<u8>) -> io::Result<bool> {
    loop {
        let line = read_crlf_line(stream).await?;
        let size_text = std::str::from_utf8(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default(), 16)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if size == 0 {
            loop {
                if read_crlf_line(stream).await?.is_empty() {
                    return Ok(true);
                }
            }
        }
        if body.len().saturating_add(size) > MAX_CAPTURE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized test upload",
            ));
        }
        if !read_exact_body(stream, body, size).await? {
            return Ok(false);
        }
        let mut crlf = [0; 2];
        stream.read_exact(&mut crlf).await?;
        if crlf != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk ending",
            ));
        }
    }
}

async fn read_exact_body(
    stream: &mut UnixStream,
    body: &mut Vec<u8>,
    mut remaining: usize,
) -> io::Result<bool> {
    let mut buffer = vec![0_u8; 16 * 1024];
    while remaining > 0 {
        let limit = remaining.min(buffer.len());
        let count = stream.read(&mut buffer[..limit]).await?;
        if count == 0 {
            return Ok(false);
        }
        body.extend_from_slice(&buffer[..count]);
        remaining -= count;
    }
    Ok(true)
}

async fn read_crlf_line(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        if line.len() >= 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized chunk line",
            ));
        }
        if stream.read(&mut byte).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short chunk line",
            ));
        }
        line.push(byte[0]);
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

pub(super) async fn write_response(
    stream: &mut UnixStream,
    status: u16,
    body: &[u8],
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_reason(status),
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

const fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}
