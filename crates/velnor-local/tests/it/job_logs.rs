// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use velnor_local::io::job_logs::save;

use crate::support::temp_dir;

const LOG: &str = "step one\n{\"legs\": {}, \"probe\": \"cache-keys\"}\n";

async fn fake_stub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .to_string();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0_u8];
                    if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&head).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = if path == "/repos/tailrocks/velnor-lab/actions/jobs/7/logs" {
                    ("200 OK", LOG)
                } else {
                    ("404 Not Found", "{\"message\": \"Not Found\"}")
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).await.ok();
            });
        }
    });
    address
}

#[tokio::test]
async fn a_job_log_from_the_stub_is_saved_under_logs_jobs_and_a_job_without_one_is_reported() {
    let address = fake_stub().await;
    let logs = temp_dir("job-logs");
    let missing = save(&address, "token", "tailrocks/velnor-lab", 5, &[7, 8], &logs)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(missing, [8]);
    let saved =
        std::fs::read_to_string(logs.join("jobs/5-7.log")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(saved, LOG);
    assert!(!logs.join("jobs/5-8.log").exists());
}
