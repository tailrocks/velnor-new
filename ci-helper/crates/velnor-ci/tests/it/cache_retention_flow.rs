// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[test]
fn five_push_saves_leave_only_the_newest_two_entries_through_the_trim_cli() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let retained = Arc::new(Mutex::new(Vec::<u64>::new()));
    let state = Arc::clone(&retained);
    let server = std::thread::spawn(move || {
        let mut pushes = 0_u64;
        for connection in listener.incoming().take(8) {
            let mut stream = connection.unwrap_or_else(|e| unreachable!("{e}"));
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap_or_default(), 1);
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request);
            let mut ids = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let body = if request.starts_with("GET ") {
                assert!(
                    request.contains("key=base-&ref=refs%2Fheads%2Fmain"),
                    "{request}"
                );
                pushes += 1;
                ids.push(pushes);
                let entries = ids.iter().map(|id| format!("{{\"id\":{id},\"key\":\"base-{id}\",\"created_at\":\"2026-11-{id:02}T00:00:00Z\"}}"))
                    .collect::<Vec<_>>().join(",");
                format!("{{\"actions_caches\":[{entries}]}}")
            } else {
                assert!(request.starts_with("DELETE "), "{request}");
                let id = request
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| path.rsplit('/').next())
                    .and_then(|text| text.parse::<u64>().ok())
                    .unwrap_or_default();
                assert!(ids.contains(&id), "{request}");
                ids.retain(|kept| *kept != id);
                String::new()
            };
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    let repo = Repo::new("cache-retention-flow");
    for push in 1..=5 {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["cache", "trim", "--prefix", "base-"])
            .env("GITHUB_REPOSITORY", "owner/name")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_TOKEN", "test-token")
            .env("GITHUB_API_URL", format!("http://{address}"))
            .current_dir(repo.root())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        let ids = retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            *ids,
            (1..=push)
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        );
    }
    server
        .join()
        .unwrap_or_else(|_| unreachable!("cache server failed"));
}
