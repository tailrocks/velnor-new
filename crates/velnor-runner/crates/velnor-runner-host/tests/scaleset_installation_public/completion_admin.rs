// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::TestResult;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use velnor_runner_host::CompletionAdminClient;

#[test]
fn refreshed_admin_session_keeps_url_and_token_together() -> TestResult<()> {
    if let Ok(log) = std::env::var("VELNOR_ADMIN_FIXTURE_LOG") {
        return exercise(&log);
    }
    let dir = tempfile::tempdir()?;
    let curl = dir.path().join("curl");
    fs::write(
        &curl,
        r#"#!/bin/sh
config=$(/bin/cat)
printf '%s\n---\n' "$config" >> "$VELNOR_ADMIN_FIXTURE_LOG"
case "$config" in
  *"https://a.example.test/old/"*"Authorization: Bearer token-a"*|*"https://b.example.test/new/"*"Authorization: Bearer token-b"*) ;;
  *) exit 1 ;;
esac
if [ ! -e "$VELNOR_ADMIN_FIXTURE_LOG.started" ]; then
  : > "$VELNOR_ADMIN_FIXTURE_LOG.started"
  while [ ! -e "$VELNOR_ADMIN_FIXTURE_LOG.release" ]; do /bin/sleep 0.01; done
fi
case "$config" in
  *'request = "DELETE"'*) printf '204' ;;
  *'request = "GET"'*) printf '{"count":0,"value":[]}200' ;;
  *) exit 1 ;;
esac
"#,
    )?;
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
    let log = dir.path().join("requests");

    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "completion_admin::refreshed_admin_session_keeps_url_and_token_together",
            "--nocapture",
        ])
        .env("PATH", dir.path())
        .env("VELNOR_ADMIN_FIXTURE_LOG", &log)
        .output()?;
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn exercise(log: &str) -> TestResult<()> {
    let client = CompletionAdminClient::new("https://a.example.test/old", "token-a")?;
    let mut old = client.session()?;
    let handle = std::thread::spawn(move || {
        let result = old.lookup("worker");
        (old, result)
    });
    let started = format!("{log}.started");
    for _ in 0..500 {
        if std::path::Path::new(&started).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(std::path::Path::new(&started).exists());
    client.refresh("https://b.example.test/new", "token-b")?;
    fs::write(format!("{log}.release"), b"")?;
    let (mut old, first) = handle.join().map_err(|_| "lookup thread panicked")?;
    assert!(first?.is_none());
    assert!(client.refresh("http://insecure.test", "bad").is_err());
    assert!(client.refresh("https://b.example.test/new", "").is_err());
    assert!(
        client
            .refresh("https://b.example.test/new", "bad\nheader")
            .is_err()
    );
    assert!(client.refresh("https://user@bad.test", "bad").is_err());
    assert!(client.refresh("https://@bad.test", "bad").is_err());
    assert!(old.lookup("worker")?.is_none());
    old.remove(7)?;
    assert!(old.lookup("worker")?.is_none());
    let mut new = client.session()?;
    assert!(new.lookup("worker")?.is_none());
    let requests = fs::read_to_string(log)?;
    let requests = requests
        .split("\n---\n")
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 5);
    for request in &requests[..4] {
        assert!(request.contains("https://a.example.test/old/"));
        assert!(request.contains("Authorization: Bearer token-a"));
        assert!(!request.contains("token-b"));
    }
    assert!(requests[4].contains("https://b.example.test/new/"));
    assert!(requests[4].contains("Authorization: Bearer token-b"));
    Ok(())
}
