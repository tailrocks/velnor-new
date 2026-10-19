use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub(super) struct CapturedRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) body: Vec<u8>,
}

pub(super) async fn read_http_request(stream: &mut UnixStream) -> Result<CapturedRequest, String> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
        let mut chunk = [0_u8; 4096];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 || bytes.len() + count > 128 * 1024 {
            return Err("Docker request headers are incomplete".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|error| error.to_string())?;
    let mut lines = header.lines();
    let mut first = lines
        .next()
        .ok_or_else(|| "Docker request line is absent".to_owned())?
        .split_whitespace();
    let method = first
        .next()
        .ok_or_else(|| "Docker method is absent".to_owned())?
        .to_owned();
    let target = first
        .next()
        .ok_or_else(|| "Docker target is absent".to_owned())?
        .to_owned();
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| {
            value
                .trim()
                .parse::<usize>()
                .map_err(|error| error.to_string())
        })
        .transpose()?
        .unwrap_or(0);
    if length > 128 * 1024 {
        return Err("Docker request body exceeds the limit".to_owned());
    }
    while bytes.len() < header_end + length {
        let mut chunk = [0_u8; 4096];
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Docker request body is incomplete".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(CapturedRequest {
        method,
        target,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

pub(super) fn normalized_route(target: &str) -> String {
    let path = target.split('?').next().unwrap_or(target);
    if path.starts_with("/v") && path.as_bytes().get(2).is_some_and(u8::is_ascii_digit) {
        path[1..]
            .find('/')
            .map_or(path, |index| &path[index + 1..])
            .to_owned()
    } else {
        path.to_owned()
    }
}

pub(super) async fn write_http_response(
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
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(body)
        .await
        .map_err(|error| error.to_string())
}
