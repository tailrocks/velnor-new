// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use velnor_ci::io::{Body, Https, Request, Transport};

type Captured = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

fn serve(answers: Vec<&'static str>) -> (u16, Captured, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let port = listener.local_addr().map_or(0, |address| address.port());
    let captured: Captured = Arc::default();
    let seen = Arc::clone(&captured);
    let handle = thread::spawn(move || {
        for answer in answers {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let (head, body) = read_request(&mut stream);
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((head, body));
            drop(stream.write_all(answer.as_bytes()));
        }
    });
    (port, captured, handle)
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        let count = stream.read(&mut chunk).unwrap_or(0);
        data.extend_from_slice(&chunk[..count]);
        if let Some(at) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        if count == 0 {
            break data.len();
        }
    };
    let head = String::from_utf8_lossy(&data[..split]).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < split + length {
        let count = stream.read(&mut chunk).unwrap_or(0);
        if count == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..count]);
    }
    (head, data[split..].to_vec())
}

fn call(port: u16, method: &'static str, body: Body<'_>) -> velnor_ci::io::Response {
    Https::new()
        .send(Request {
            method,
            url: format!("http://127.0.0.1:{port}/v2/x"),
            headers: vec![("Authorization", "Bearer abc".to_owned())],
            body,
        })
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_streamed_blob_is_sent_with_its_length_and_the_headers_come_back_lowercase() {
    let reply = "HTTP/1.1 201 Created\r\nDocker-Content-Digest: sha256:x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let (port, captured, handle) = serve(vec![reply]);
    let reader = Cursor::new(b"blob-bytes".to_vec());
    let response = call(
        port,
        "PUT",
        Body::Stream {
            reader: Box::new(reader),
            length: 10,
        },
    );
    assert_eq!(response.status, 201);
    assert_eq!(response.header("docker-content-digest"), Some("sha256:x"));
    drop(handle.join());
    let seen = captured
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (head, body) = &seen[0];
    assert!(head.starts_with("put /v2/x http/1.1"), "{head}");
    assert!(head.contains("content-length: 10\r\n"), "{head}");
    assert!(!head.contains("transfer-encoding"), "{head}");
    assert!(head.contains("authorization: bearer abc\r\n"), "{head}");
    assert_eq!(body, b"blob-bytes");
}

#[test]
fn an_empty_post_declares_a_zero_length() {
    let reply = "HTTP/1.1 202 Accepted\r\nLocation: /v2/u\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let (port, captured, handle) = serve(vec![reply]);
    let response = call(port, "POST", Body::Bytes(&[]));
    assert_eq!(response.status, 202);
    assert_eq!(response.header("location"), Some("/v2/u"));
    drop(handle.join());
    let seen = captured
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(seen[0].0.contains("content-length: 0\r\n"), "{}", seen[0].0);
}

#[test]
fn a_redirect_and_an_error_status_are_returned_and_never_followed() {
    let redirect = "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let missing = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let (port, captured, handle) = serve(vec![redirect, missing]);
    let moved = call(port, "GET", Body::Empty);
    assert_eq!(moved.status, 307);
    assert_eq!(
        moved.header("location"),
        Some("http://127.0.0.1:1/elsewhere")
    );
    assert_eq!(call(port, "HEAD", Body::Empty).status, 404);
    drop(handle.join());
    assert_eq!(
        captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        2
    );
}
