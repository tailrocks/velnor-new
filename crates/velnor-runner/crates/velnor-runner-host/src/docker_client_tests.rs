//! Bollard is constructed only from the configured absolute Unix socket.

use std::path::Path;
use std::time::Duration;

use crate::docker_client::docker_deadline_after;
use crate::{HostError, connect_unix};

fn constructor_is_unix(tail: &str) -> bool {
    let Some(rest) = tail.strip_prefix("connect_with_unix") else {
        return false;
    };
    match rest.chars().next() {
        Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => false,
        Some(_) | None => true,
    }
}

fn assert_rejected(socket: &str) {
    assert!(matches!(connect_unix(socket), Err(HostError::Docker)));
}

#[test]
fn rejected_sockets_do_not_need_a_daemon() {
    assert_rejected("tcp://127.0.0.1:2375");
    assert_rejected("TCP://127.0.0.1:2375");
    assert_rejected("unix://var/run/docker.sock");
    assert_rejected("relative.sock");
    assert_rejected("");
}

#[test]
fn source_uses_only_the_unix_constructor() -> Result<(), std::io::Error> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let source = std::fs::read_to_string(Path::new(&manifest).join("src/docker_client.rs"))?;
    let banned = format!("{}{}", "connect_with_", "local_defaults");
    assert!(!source.contains(&banned));
    let mut rest = source.as_str();
    let mut saw = false;
    while let Some(index) = rest.find("connect_with_") {
        let tail = &rest[index..];
        assert!(constructor_is_unix(tail), "unexpected bollard constructor");
        saw = true;
        rest = &tail["connect_with_".len()..];
    }
    assert!(saw);
    Ok(())
}

#[tokio::test]
async fn docker_deadline_preserves_results_and_fails_closed_on_timeout() {
    let response =
        docker_deadline_after(async { Ok::<_, u16>(200) }, Duration::from_millis(50)).await;
    assert_eq!(response, Ok(Ok(200)));

    let timeout = docker_deadline_after(
        tokio::time::sleep(Duration::from_millis(20)),
        Duration::from_millis(1),
    )
    .await;
    assert_eq!(timeout, Err(HostError::Docker));
}
