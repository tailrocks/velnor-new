// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn shared_deadline_bounds_stalled_graphql_body() {
    crate::api::github_evidence_http::agent().expect("TLS setup");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).expect("read") > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{")
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
    let start = Instant::now();
    let result = deadline.scope(|| client.qualification_review_threads("owner/repo", 1, None));
    let elapsed = start.elapsed();
    let _released = release.send(());
    worker.join().expect("server");
    assert!(!matches!(result, Ok(Ok(_))));
    assert!(
        elapsed < Duration::from_secs(6),
        "GraphQL exceeded shared deadline"
    );
}
