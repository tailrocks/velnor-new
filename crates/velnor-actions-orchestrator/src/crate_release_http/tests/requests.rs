// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn reads_authenticated_json_and_does_not_follow_redirects() {
    let (origin, worker) = server(vec![reply("200 OK", r#"{"ok":true}"#)]);
    let response = client(origin)
        .get("repos/example/demo-crate")
        .expect("authenticated JSON");
    assert_eq!(response["ok"], true);
    let requests = worker.join().expect("server");
    let request = requests[0].to_ascii_lowercase();
    assert!(request.contains("authorization: bearer synthetic-test-credential"));
    assert!(request.contains("accept: application/vnd.github+json"));
    assert!(request.contains("x-github-api-version: 2022-11-28"));
    let sink = TcpListener::bind("127.0.0.1:0").expect("redirect sink");
    sink.set_nonblocking(true).expect("nonblocking");
    let location = format!("http://{}/stolen", sink.local_addr().expect("address"));
    let (origin, worker) = server(vec![format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )]);
    assert!(client(origin).get("repos/example/demo-crate").is_err());
    worker.join().expect("server");
    assert_eq!(
        sink.accept()
            .expect_err("redirect must not be requested")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn accepts_encoded_branch_and_check_pagination() {
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Encoding: identity\r\nConnection: close\r\n\r\n{}";
    let (origin, worker) = server(vec![response.to_owned(), reply("200 OK", "{}")]);
    let client = client(origin);
    assert!(
        client
            .get("repos/example/demo-crate/git/ref/heads/release%2Fmain")
            .is_ok()
    );
    assert!(client.get("repos/example/demo-crate/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/check-runs?filter=all&per_page=100&page=2").is_ok());
    assert_eq!(worker.join().expect("server").len(), 2);
}

#[test]
fn accepts_same_run_artifact_metadata_routes() {
    let (origin, worker) = server(vec![reply("200 OK", "{}"), reply("200 OK", "{}")]);
    let client = client(origin);
    assert!(
        client
            .get("repos/example/demo-crate/actions/runs/123/artifacts?per_page=100&page=1")
            .is_ok()
    );
    assert!(
        client
            .get("repos/example/demo-crate/actions/artifacts/456")
            .is_ok()
    );
    assert_eq!(worker.join().expect("server").len(), 2);
}

#[test]
fn posts_exact_approved_tag_and_release() {
    let tag_response = r#"{"ref":"refs/tags/demo-crate-v0.1.1","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
    let release_response = r#"{"tag_name":"demo-crate-v0.1.1","target_commitish":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","draft":false,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#;
    let (origin, worker) = server(vec![
        reply("201 Created", tag_response),
        reply("201 Created", release_response),
    ]);
    let client = client(origin);
    assert!(
        client
            .create_tag(
                "example/demo-crate",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .is_ok()
    );
    assert!(
        client
            .create_release(
                "example/demo-crate",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .is_ok()
    );
    let requests = worker.join().expect("server");
    assert!(requests[0].starts_with("POST /repos/example/demo-crate/git/refs "));
    assert!(requests[1].starts_with("POST /repos/example/demo-crate/releases "));
    let tag_body: serde_json::Value =
        serde_json::from_str(requests[0].split_once("\r\n\r\n").expect("body").1)
            .expect("JSON body");
    let release_body: serde_json::Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").expect("body").1)
            .expect("JSON body");
    assert_eq!(
        tag_body,
        serde_json::json!({"ref":"refs/tags/demo-crate-v0.1.1", "sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})
    );
    assert_eq!(
        release_body,
        serde_json::json!({"tag_name":"demo-crate-v0.1.1", "name":"demo-crate-v0.1.1", "target_commitish":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "make_latest":"false"})
    );
}

#[test]
fn rejects_invalid_writes_before_network() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    listener.set_nonblocking(true).expect("nonblocking");
    let client = client(format!(
        "http://{}",
        listener.local_addr().expect("address")
    ));
    for (repo, tag, sha) in [
        (
            "bad/repo/extra",
            "demo-crate-v0.1.1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        (
            "example/demo-crate",
            "../escape",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        (
            "example/demo-crate",
            "v1.lock",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        ("example/demo-crate", "v1", "not-a-sha"),
    ] {
        let result = client.create_tag(repo, tag, sha);
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains(TOKEN));
        assert!(client.create_release(repo, tag, sha).is_err());
    }
    assert_eq!(
        listener
            .accept()
            .expect_err("invalid write must not send")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn rejects_non_created_and_wrong_created_responses() {
    for status in [
        "200 OK",
        "202 Accepted",
        "301 Moved Permanently",
        "302 Found",
        "401 Unauthorized",
        "403 Forbidden",
        "409 Conflict",
        "422 Unprocessable Entity",
        "429 Too Many Requests",
        "500 Server Error",
    ] {
        for release in [false, true] {
            let (origin, worker) = server(vec![reply(status, "{}")]);
            let client = client(origin);
            let result = if release {
                client.create_release(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
            } else {
                client.create_tag(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
            };
            assert!(result.is_err(), "accepted {status}");
            assert!(!format!("{result:?}").contains(TOKEN));
            assert_eq!(worker.join().expect("server").len(), 1);
        }
    }
    for body in [
        "not-json",
        r#"{"ref":"refs/tags/demo-crate-v0.1.1","object":{"type":"commit","sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}"#,
        r#"{"ref":"refs/tags/other","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#,
    ] {
        let (origin, worker) = server(vec![reply("201 Created", body)]);
        assert!(
            client(origin)
                .create_tag(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_err()
        );
        worker.join().expect("server");
    }
    for body in [
        r#"{"tag_name":"other","draft":false,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#,
        r#"{"tag_name":"demo-crate-v0.1.1","draft":true,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#,
        r#"{"tag_name":"demo-crate-v0.1.1","draft":false,"html_url":"https://evil.test/"}"#,
    ] {
        let (origin, worker) = server(vec![reply("201 Created", body)]);
        assert!(
            client(origin)
                .create_release(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_err()
        );
        worker.join().expect("server");
    }
}

#[test]
fn ambiguous_post_failure_is_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let size = stream.read(&mut buffer).expect("read");
            assert!(size > 0);
            request.extend_from_slice(&buffer[..size]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|n| n.trim().parse::<usize>().ok())
                    })
                    .expect("content length");
                if request.len() >= index + 4 + length {
                    break;
                }
            }
        }
        drop(stream);
        listener.set_nonblocking(true).expect("nonblocking");
        thread::sleep(Duration::from_millis(500));
        assert_eq!(
            listener
                .accept()
                .expect_err("POST must not be retried")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let result = client(format!("http://{address}")).create_tag(
        "example/demo-crate",
        "demo-crate-v0.1.1",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert!(result.is_err());
    assert!(!format!("{result:?}").contains(TOKEN));
    worker.join().expect("server");
}

#[test]
fn post_redirect_does_not_forward_credentials() {
    let sink = TcpListener::bind("127.0.0.1:0").expect("redirect sink");
    sink.set_nonblocking(true).expect("nonblocking");
    let location = format!("http://{}/stolen", sink.local_addr().expect("address"));
    let (origin, worker) = server(vec![format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )]);
    assert!(
        client(origin)
            .create_tag(
                "example/demo-crate",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .is_err()
    );
    worker.join().expect("server");
    assert_eq!(
        sink.accept()
            .expect_err("redirect must not be requested")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn post_rejects_ambiguous_headers_and_truncated_response() {
    for response in [
            "HTTP/1.1 201 Created\r\nContent-Length: 2\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{}".to_owned(),
            "HTTP/1.1 201 Created\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".to_owned(),
            "HTTP/1.1 201 Created\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_owned(),
        ] {
            let (origin, worker) = server(vec![response]);
            assert!(client(origin).create_tag("example/demo-crate", "demo-crate-v0.1.1", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
            worker.join().expect("server");
        }
}

#[test]
fn post_deadline_bounds_stalled_response() {
    crate::api::github_evidence_http::agent().expect("TLS setup");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).expect("read") > 0);
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 20\r\n\r\n{")
            .expect("headers");
        thread::sleep(Duration::from_secs(3));
    });
    let client = GitHubAdmissionHttp {
        token: TOKEN.to_owned(),
        timeout: Duration::from_secs(2),
        origin: format!("http://{address}"),
        responses: std::cell::RefCell::default(),
    };
    assert!(
        client
            .create_tag(
                "example/demo-crate",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .is_err()
    );
    worker.join().expect("server");
}

#[test]
fn shared_deadline_bounds_stalled_journal_request() {
    crate::api::github_evidence_http::agent().expect("TLS setup");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).expect("read") > 0);
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 20\r\n\r\n{")
            .expect("headers");
        let _held = hold.recv_timeout(Duration::from_secs(12));
    });
    let client = GitHubAdmissionHttp {
        token: TOKEN.to_owned(),
        timeout: Duration::from_secs(10),
        origin: format!("http://{address}"),
        responses: std::cell::RefCell::default(),
    };
    let deadline =
        velnor_actions_mise::CheckDeadline::after(Duration::from_secs(1)).expect("deadline");
    let start = std::time::Instant::now();
    let result = deadline.scope(|| {
        client.create_tag(
            "example/demo-crate",
            "demo-crate-v0.1.1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
    });
    let elapsed = start.elapsed();
    let _released = release.send(());
    worker.join().expect("server");
    assert!(!matches!(result, Ok(Ok(()))));
    assert!(
        elapsed < Duration::from_secs(6),
        "request exceeded shared deadline"
    );
}

#[test]
fn aged_client_gets_fresh_bounded_request() -> Result<(), Box<dyn std::error::Error>> {
    let (origin, worker) = server(vec![reply("200 OK", r#"{"ok":true}"#)]);
    let mut client = client(origin);
    client.timeout = Duration::from_secs(1);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(client.get("repos/acme/repo")?["ok"], true);
    worker.join().map_err(|_| "server")?;
    Ok(())
}

#[test]
fn request_timeout_must_be_nonzero_and_bounded() {
    assert!(GitHubAdmissionHttp::new(TOKEN.into(), Duration::MAX).is_err());
    assert!(GitHubAdmissionHttp::new(TOKEN.into(), Duration::ZERO).is_err());
    assert!(GitHubAdmissionHttp::new(TOKEN.into(), Duration::from_secs(180)).is_ok());
}
