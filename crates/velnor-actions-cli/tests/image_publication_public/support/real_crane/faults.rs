// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{self, Read, Write},
    net::TcpStream,
    sync::Mutex,
};

use super::{
    ManifestFault, REPOSITORY_PREFIX, State,
    http::{Request, Response, response},
};

const MAX_TLS_RECORD_BYTES: usize = 18 * 1024;

pub(super) fn reject_tls_probe(stream: &mut TcpStream) -> io::Result<bool> {
    let mut first = [0; 1];
    if stream.peek(&mut first)? != 1 || first[0] != 0x16 {
        return Ok(false);
    }
    let mut header = [0; 5];
    stream.read_exact(&mut header)?;
    let record_size = usize::from(u16::from_be_bytes([header[3], header[4]]));
    if record_size > MAX_TLS_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TLS probe record exceeds fixture limit",
        ));
    }
    let mut record = vec![0; record_size];
    stream.read_exact(&mut record)?;
    stream
        .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    stream.flush()?;
    Ok(true)
}

pub(super) fn should_drop_manifest_lookup(
    request: &Request,
    state: &Mutex<State>,
) -> io::Result<bool> {
    if !matches!(request.method.as_str(), "GET" | "HEAD")
        || !request
            .path
            .strip_prefix(REPOSITORY_PREFIX)
            .is_some_and(|path| path.starts_with("manifests/"))
    {
        return Ok(false);
    }
    let state = state
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(state.manifest_fault == ManifestFault::Transport)
}

pub(super) fn unauthorized_manifest() -> Response {
    response(
        401,
        "Unauthorized",
        vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            (
                "Www-Authenticate".to_owned(),
                "Basic realm=\"native crane fixture\"".to_owned(),
            ),
        ],
        b"{\"errors\":[{\"code\":\"UNAUTHORIZED\",\"message\":\"fixture auth required\"}]}"
            .to_vec(),
    )
}
