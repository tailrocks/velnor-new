use tokio::{io::AsyncReadExt, net::UnixStream};

use super::super::CapturedRequest;

const HTTP_LIMIT: usize = 128 * 1024;

pub(super) async fn read_request(stream: &mut UnixStream) -> Result<CapturedRequest, String> {
    let mut bytes = Vec::new();
    let header_end = read_headers(stream, &mut bytes).await?;
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "Docker request headers are not UTF-8".to_owned())?;
    let mut lines = header.lines();
    let mut request = lines
        .next()
        .ok_or_else(|| "Docker request line is absent".to_owned())?
        .split_whitespace();
    let method = request
        .next()
        .ok_or_else(|| "Docker request method is absent".to_owned())?
        .to_owned();
    let target = request
        .next()
        .ok_or_else(|| "Docker request path is absent".to_owned())?
        .to_owned();
    let length = content_length(lines)?;
    if header_end + length > HTTP_LIMIT {
        return Err("Docker API request exceeds the test limit".to_owned());
    }
    read_body(stream, &mut bytes, header_end + length).await?;
    Ok(CapturedRequest {
        method,
        target,
        body: bytes[header_end..header_end + length].to_vec(),
        response_status: None,
        response_body: Vec::new(),
    })
}

async fn read_headers(stream: &mut UnixStream, bytes: &mut Vec<u8>) -> Result<usize, String> {
    loop {
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            return Ok(index + 4);
        }
        if bytes.len() >= HTTP_LIMIT {
            return Err("Docker API headers exceed the test limit".to_owned());
        }
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Docker closed before sending request headers".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

async fn read_body(
    stream: &mut UnixStream,
    bytes: &mut Vec<u8>,
    full_length: usize,
) -> Result<(), String> {
    while bytes.len() < full_length {
        let mut chunk = [0_u8; 2048];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Docker closed before sending the request body".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(())
}

fn content_length<'a>(lines: impl Iterator<Item = &'a str>) -> Result<usize, String> {
    for line in lines {
        if let Some((name, value)) = line.trim_end_matches('\r').split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            return value
                .trim()
                .parse()
                .map_err(|_| "Docker content length is malformed".to_owned());
        }
    }
    Ok(0)
}
