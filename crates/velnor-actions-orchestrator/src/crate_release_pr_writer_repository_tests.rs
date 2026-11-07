// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

fn repository_response(value: serde_json::Value) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("address");
    let worker = thread::spawn(move || {
        let ordinary =
            serde_json::json!({"head":{"ref":"feature","repo":{"full_name":"owner/project"}}});
        let pages = [serde_json::json!(vec![ordinary; 100]), value];
        for page in pages.iter().cycle().take(4) {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read");
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            assert!(bytes.starts_with(b"GET "));
            let body = page.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write");
        }
    });
    (format!("http://{address}"), worker)
}

#[test]
fn release_pr_recognition_and_competition_require_repository_identity()
-> Result<(), OrchestratorError> {
    let cases = [
        (Some("other/fork"), "release-plz-another", true, true),
        (Some("owner/project"), "release-plz-another", false, false),
        (Some("OWNER/PROJECT"), "release-plz-123-1", true, false),
        (Some("owner/project"), "feature", true, true),
        (None, "release-plz-another", false, false),
        (Some(""), "release-plz-another", false, false),
        (Some("malformed"), "release-plz-another", false, false),
    ];
    for (repository, branch, can_prepare, source_is_regular) in cases {
        let rows = serde_json::json!([{"head":{"ref":branch,"repo":{"full_name":repository}}}]);
        let (endpoint, worker) = repository_response(rows);
        let github = crate::crate_release_github::NativeGitHubAdmissionRead {
            reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&endpoint)?,
        };
        let writer = NativeWriter { github: &github };
        let preparation = writer.require_no_other_release_pr(&plan());
        let recognition = writer.require_not_release_pr(&plan());
        worker.join().expect("worker");
        assert_eq!(
            preparation.is_ok(),
            can_prepare,
            "competition: {repository:?}/{branch}"
        );
        assert_eq!(
            recognition.is_ok(),
            source_is_regular,
            "recognition: {repository:?}/{branch}"
        );
    }
    Ok(())
}
