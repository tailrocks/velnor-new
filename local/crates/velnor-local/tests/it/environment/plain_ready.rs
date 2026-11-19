// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::io::env::Environment;

use super::{Published, plain};
use crate::support::temp_dir;

async fn runner_service(
    answer: fn(&str, bool) -> String,
    online_after: std::time::Duration,
) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let online_at = tokio::time::Instant::now() + online_after;
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0_u8; 2048];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let body = answer(&path, tokio::time::Instant::now() >= online_at);
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes()).await;
        }
    });
    address.to_string()
}

fn online_when_ready(path: &str, ready: bool) -> String {
    if path.contains("isagentonline") {
        format!("{{\"online\":{ready}}}")
    } else {
        "{\"value\":[]}".to_owned()
    }
}

#[tokio::test]
async fn up_waits_until_the_plain_runner_reports_online_not_just_http_200() {
    let dir = temp_dir("environment-plain-online");
    let delay = std::time::Duration::from_millis(700);
    let started = tokio::time::Instant::now();
    let address = runner_service(online_when_ready, delay).await;
    let env = Environment::at(&dir, Published(address))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let up = env.up(&dir, &plain(&dir, 1, 0)).await;
    assert!(up.is_ok(), "{:?}", up.err());
    assert!(
        started.elapsed() >= delay,
        "up returned while the runner was offline"
    );
}

async fn up_error(answer: fn(&str, bool) -> String, runners: (u16, u16)) -> String {
    let dir = temp_dir("environment-plain-offline");
    let address = runner_service(answer, std::time::Duration::from_secs(3600)).await;
    let env = Environment::at(&dir, Published(address))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .with_ready_timeout(std::time::Duration::from_millis(1500));
    let limit = std::time::Duration::from_secs(30);
    let args = plain(&dir, runners.0, runners.1);
    let up = tokio::time::timeout(limit, env.up(&dir, &args)).await;
    up.unwrap_or_else(|_| unreachable!("up hung past the readiness deadline"))
        .err()
        .unwrap_or_else(|| unreachable!("up succeeded with an offline runner"))
        .to_string()
}

#[tokio::test]
async fn up_fails_with_a_clear_reason_when_a_plain_runner_stays_offline() {
    let error = up_error(online_when_ready, (1, 0)).await;
    assert!(
        error.contains("runner 1") && error.contains("offline"),
        "{error}"
    );
}

#[tokio::test]
async fn up_fails_when_the_online_answer_is_malformed_or_missing() {
    for (answer, state) in [
        (
            (|_: &str, _: bool| "not json".to_owned()) as fn(&str, bool) -> String,
            "malformed answer",
        ),
        (
            |path, _| {
                if path.contains("isagentonline") {
                    "{}".to_owned()
                } else {
                    "{\"value\":[]}".to_owned()
                }
            },
            "no boolean `online`",
        ),
    ] {
        let error = up_error(answer, (1, 0)).await;
        assert!(
            error.contains("runner 1") && error.contains(state),
            "{error}"
        );
    }
}

#[tokio::test]
async fn up_requires_every_trusted_and_fork_plain_runner_online() {
    fn only_trusted(path: &str, _: bool) -> String {
        if path.contains("isagentonline") {
            format!("{{\"online\":{}}}", !path.contains("fork-runner"))
        } else {
            "{\"value\":[]}".to_owned()
        }
    }
    let error = up_error(only_trusted, (1, 1)).await;
    assert!(error.contains("fork-runner 1"), "{error}");
}
