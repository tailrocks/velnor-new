// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rejects_unsafe_routes_without_sending_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    listener.set_nonblocking(true).expect("nonblocking");
    let client = client(format!(
        "http://{}",
        listener.local_addr().expect("address")
    ));
    for route in [
        "https://evil.test/",
        "repos/example/demo-crate/%2E%2E/secrets",
        "repos/example/demo-crate/git/ref/tags/%252e%252e",
        "repos/example/demo-crate/git/ref/tags/a%0D%0AInjected",
        "repos/example/demo-crate?x=1",
        "repos/example/demo-crate/git/ref/tags/a%2F..%2Fsecret",
        "users/name@evil.test",
    ] {
        let result = client.get(route);
        assert!(result.is_err(), "accepted {route}");
        assert!(!format!("{result:?}").contains(TOKEN));
    }
    assert_eq!(
        listener
            .accept()
            .expect_err("invalid routes must not send")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(!format!("{client:?}").contains(TOKEN));
}

#[test]
fn rejects_errors_malformed_and_oversized_bodies() {
    for status in [
        "401 Unauthorized",
        "403 Forbidden",
        "404 Not Found",
        "429 Too Many Requests",
        "500 Server Error",
    ] {
        let (origin, worker) = server(vec![reply(status, "{}")]);
        let result = client(origin).get("repos/example/demo-crate");
        assert!(result.is_err(), "accepted {status}");
        assert!(!format!("{result:?}").contains(TOKEN));
        worker.join().expect("server");
    }
    for response in [reply("200 OK", "not-json"), "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_owned(), "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n{}".to_owned(), "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n{}".to_owned()] {
            let (origin, worker) = server(vec![response]);
            assert!(client(origin).get("repos/example/demo-crate").is_err());
            worker.join().expect("server");
        }
}

#[test]
fn bounds_streamed_json_without_content_length() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0; 8192];
        assert!(stream.read(&mut buffer).expect("read") > 0);
        let body = format!("\"{}\"", "x".repeat(8 * 1024 * 1024));
        let headers = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n",
            body.len()
        );
        let _written = stream
            .write_all(headers.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .and_then(|()| stream.write_all(b"\r\n0\r\n\r\n"));
    });
    assert!(
        client(format!("http://{address}"))
            .get("repos/example/demo-crate")
            .is_err()
    );
    worker.join().expect("server");
}

#[test]
fn journal_ref_read_brackets_absence_with_repository_access() {
    use crate::{
        crate_release_github::NativeGitHubAdmissionRead, crate_release_journal::JournalIo as _,
        crate_release_journal_adapter::JournalAdapter,
    };
    let repo = r#"{"full_name":"example/demo-crate","default_branch":"main"}"#;
    let branch = r#"{"ref":"refs/heads/main","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
    let reference = "refs/tags/velnor-receipt/12/1/publication-started";
    let cases = [
        (
            vec![
                reply("200 OK", repo),
                reply("200 OK", branch),
                reply("404 Not Found", "{}"),
                reply("200 OK", repo),
                reply("200 OK", branch),
            ],
            true,
        ),
        (vec![reply("403 Forbidden", "{}")], false),
        (
            vec![
                reply("200 OK", repo),
                reply("200 OK", branch),
                reply("404 Not Found", "{}"),
                reply("403 Forbidden", "{}"),
            ],
            false,
        ),
    ];
    for (responses, succeeds) in cases {
        let count = responses.len();
        let (origin, worker) = server(responses);
        let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
            reader: client(origin),
        });
        let result = adapter.read_ref_with_access("example/demo-crate", reference);
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert_eq!(result.expect("authorized absence"), None);
        }
        assert_eq!(worker.join().expect("server").len(), count);
    }
    let present = format!(
        r#"{{"ref":"{reference}","object":{{"type":"tag","sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}}}"#
    );
    let (origin, worker) = server(vec![
        reply("200 OK", repo),
        reply("200 OK", branch),
        reply("200 OK", &present),
    ]);
    let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
        reader: client(origin),
    });
    assert_eq!(
        adapter
            .read_ref_with_access("example/demo-crate", reference)
            .expect("authorized reference"),
        Some("b".repeat(40))
    );
    assert_eq!(worker.join().expect("server").len(), 3);
}

#[test]
fn optional_absence_requires_access_to_default_branch() {
    let repo = r#"{"full_name":"example/demo-crate","default_branch":"main"}"#;
    let branch = r#"{"ref":"refs/heads/main","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
    let (origin, worker) = server(vec![
        reply("404 Not Found", "{}"),
        reply("200 OK", repo),
        reply("200 OK", branch),
    ]);
    assert!(
        client(origin)
            .get_optional("repos/example/demo-crate/git/ref/tags/demo-crate-v0.1.1")
            .expect("authorized absence")
            .is_none()
    );
    assert_eq!(worker.join().expect("server").len(), 3);
    let (origin, worker) = server(vec![
        reply("404 Not Found", "{}"),
        reply("403 Forbidden", "{}"),
    ]);
    assert!(
        client(origin)
            .get_optional("repos/example/demo-crate/git/ref/tags/demo-crate-v0.1.1")
            .is_err()
    );
    worker.join().expect("server");
    let (origin, worker) = server(vec![
        reply("404 Not Found", "{}"),
        reply("200 OK", repo),
        reply("404 Not Found", "{}"),
    ]);
    assert!(
        client(origin)
            .get_optional("repos/example/demo-crate/releases/tags/demo-crate-v0.1.1")
            .is_err()
    );
    worker.join().expect("server");
}

#[test]
fn deadline_stops_a_stalled_response_body() {
    crate::api::github_evidence_http::agent().expect("TLS setup");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).expect("read") > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{")
            .expect("headers");
        thread::sleep(Duration::from_secs(3));
    });
    let client = GitHubAdmissionHttp {
        token: TOKEN.to_owned(),
        deadline: Instant::now() + Duration::from_secs(2),
        origin: format!("http://{address}"),
        responses: std::cell::RefCell::default(),
    };
    assert!(client.get("repos/example/demo-crate").is_err());
    worker.join().expect("server");
}

#[test]
fn qualification_job_route_accepts_only_a_numeric_job_id() {
    assert!(valid_route("repos/owner/repo/actions/jobs/42"));
    assert!(!valid_route("repos/owner/repo/actions/jobs/0"));
    assert!(!valid_route("repos/owner/repo/actions/jobs/42/logs"));
    assert!(!valid_route(
        "repos/owner/repo/actions/jobs/42?unexpected=1"
    ));
}

#[test]
fn qualification_feedback_routes_are_strictly_bounded() {
    for path in [
        "repos/owner/repo/pulls/42",
        "repos/owner/repo/pulls/42/commits?per_page=100&page=1",
        "repos/owner/repo/pulls/42/comments?per_page=100&page=1",
        "repos/owner/repo/issues/42/comments?per_page=100&page=1",
        "repos/owner/repo/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/comments?per_page=100&page=1",
    ] {
        assert!(valid_route(path), "{path}");
    }
    for path in [
        "repos/owner/repo/pulls/0",
        "repos/owner/repo/pulls/42/commits?per_page=100&page=101",
        "repos/owner/repo/issues/42/comments?per_page=101&page=1",
        "repos/owner/repo/commits/invalid/comments?per_page=100&page=1",
    ] {
        assert!(!valid_route(path), "{path}");
    }
}

#[test]
fn qualification_branch_rules_route_is_bounded() {
    assert!(valid_route(
        "repos/owner/repo/rules/branches/release%2Fmain?per_page=100&page=1"
    ));
    assert!(!valid_route(
        "repos/owner/repo/rules/branches/main?per_page=100&page=101"
    ));
    assert!(!valid_route(
        "repos/owner/repo/rules/branches/%2e%2e?per_page=100&page=1"
    ));
    assert!(!valid_route(
        "repos/owner/repo/rules/branches/main?per_page=100&page=1&other=1"
    ));
}
