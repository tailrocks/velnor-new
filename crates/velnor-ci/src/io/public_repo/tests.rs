// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use velnor_model::GitRepository;

use super::{Credentials, check, get, read, redact};

const TOKEN: &str = "ghs_unit_token_must_not_leave";

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

fn serve(status: u16, location: Option<String>, body: &'static str) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            if let Ok(mut seen) = seen.lock() {
                seen.push(String::from_utf8_lossy(&request).to_ascii_lowercase());
            }
            let mut lines = vec![format!("HTTP/1.1 {status} Reply")];
            lines.extend(
                location
                    .iter()
                    .map(|location| format!("location: {location}")),
            );
            lines.push(format!("content-length: {}", body.len()));
            lines.push("connection: close".to_owned());
            let head = format!("{}\r\n\r\n", lines.join("\r\n"));
            stream.write_all(head.as_bytes()).ok();
            stream.write_all(body.as_bytes()).ok();
        }
    });
    Server { url, requests }
}

fn repository() -> GitRepository {
    GitRepository {
        owner: "acme".to_owned(),
        name: "shared-lib".to_owned(),
    }
}

const PUBLIC: &str = r#"{"full_name":"acme/shared-lib","private":false}"#;

#[test]
fn the_token_is_sent_as_a_bearer_header_when_the_caller_passes_it() {
    let server = serve(200, None, PUBLIC);
    let result = read(
        &format!("{}/repos/acme/shared-lib", server.url),
        &repository(),
        Some(TOKEN),
        Credentials::Token,
    );
    assert_eq!(result, Ok(Credentials::Token));
    let requests = server.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(
        requests[0].contains(&format!(
            "authorization: bearer {}\r\n",
            TOKEN.to_ascii_lowercase()
        )),
        "{}",
        requests[0]
    );
}

#[test]
fn a_redirect_is_never_followed_so_the_token_cannot_reach_another_host() {
    let elsewhere = serve(200, None, PUBLIC);
    let moved = serve(
        307,
        Some(format!("{}/repos/acme/shared-lib", elsewhere.url)),
        "",
    );
    let result = read(
        &format!("{}/repos/acme/shared-lib", moved.url),
        &repository(),
        Some(TOKEN),
        Credentials::Token,
    );
    let error = result.err().unwrap_or_default();
    assert!(error.contains("HTTP 307"), "{error}");
    assert!(!error.contains(TOKEN), "{error}");
    assert_eq!(moved.requests().len(), 1);
    assert!(
        moved.requests()[0].contains("authorization: bearer"),
        "the first request must carry the token or this test proves nothing"
    );
    assert!(
        elsewhere.requests().is_empty(),
        "{:?}",
        elsewhere.requests()
    );
}

#[test]
fn a_host_other_than_the_github_api_gets_no_token_from_check() {
    let server = serve(200, None, PUBLIC);
    let result = check(&server.url, &repository(), Some(TOKEN));
    assert_eq!(result, Ok(Credentials::None));
    let requests = server.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(!requests[0].contains("authorization"), "{}", requests[0]);
    assert!(
        !requests[0].contains(&TOKEN.to_ascii_lowercase()),
        "{}",
        requests[0]
    );
}

#[test]
fn a_failed_connection_does_not_echo_the_token() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}/repos/acme/shared-lib",
        closed.local_addr().unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(closed);
    let error = get(&url, Some(TOKEN)).err().unwrap_or_default();
    assert!(error.starts_with("lookup failed"), "{error}");
    assert!(!error.contains(TOKEN), "{error}");
}

#[test]
fn a_token_that_is_not_a_valid_header_value_is_not_echoed() {
    let server = serve(200, None, PUBLIC);
    let token = "bad\ntoken-value";
    let error = get(
        &format!("{}/repos/acme/shared-lib", server.url),
        Some(token),
    )
    .err()
    .unwrap_or_default();
    assert!(error.starts_with("lookup failed"), "{error}");
    assert!(!error.contains(token), "{error}");
}

#[test]
fn a_token_in_an_error_text_is_replaced_and_other_text_is_kept() {
    let text = redact(format!("lookup failed: bad header {TOKEN}"), Some(TOKEN));
    assert_eq!(text, "lookup failed: bad header ***");
    assert_eq!(
        redact("lookup failed: timeout".to_owned(), None),
        "lookup failed: timeout"
    );
}
