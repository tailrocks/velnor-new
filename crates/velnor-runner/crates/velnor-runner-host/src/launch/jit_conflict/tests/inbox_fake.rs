// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::super::{TestResult, case::Case, github::Github};
use velnor_runner_github::{
    Exchange, Method, QueueSession, SessionRequest, Transport, TransportFail,
};

const DIRECTORY: &str = "VELNOR_JIT_INBOX_FAKE";

pub(super) fn isolated(name: &str) -> TestResult<bool> {
    if std::env::var_os(DIRECTORY).is_some() {
        return Ok(false);
    }
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("curl");
    std::fs::write(&executable, include_str!("inbox_fake.ts"))?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let mut paths = vec![directory.path().to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("PATH")?,
    ));
    let (_, name) = name.split_once("::").ok_or("qualified test name")?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", name, "--nocapture"])
        .env(DIRECTORY, directory.path())
        .env("PATH", std::env::join_paths(paths)?)
        .output()?;
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stderr().write_all(&output.stderr)?;
    assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    assert!(
        output.status.success(),
        "isolated inbox recovery test failed: {}",
        output.status
    );
    Ok(true)
}

pub(super) async fn poll(case: &Case, body: &str) -> TestResult<()> {
    let directory = PathBuf::from(std::env::var_os(DIRECTORY).ok_or("fake directory")?);
    std::fs::write(directory.join("body"), body)?;
    std::fs::write(directory.join("requests"), "")?;
    let mut admin = Github::new("unrelated", false);
    let mut link = admin.link(&case.binding)?;
    let mut session = session()?;
    let nonce = "e".repeat(32);
    super::phase(
        case.scope.prepare_session_create(&nonce).await,
        "prepare poll session",
    )?;
    let claim = super::phase(
        case.scope.claim_session_create(&nonce).await,
        "claim poll session",
    )?
    .ok_or("session claim")?;
    let bound = super::phase(
        case.scope
            .record_created_session(&claim, &session.session_id, 0)
            .await,
        "bind poll session",
    )?;
    let outcome = super::phase(
        crate::launch::poll_scoped_connection_once(
            &mut link,
            7,
            &mut session,
            &case.scope,
            bound.intent_id(),
            bound.allocation(),
            0,
        )
        .await,
        "real scoped poll",
    )?;
    assert_eq!(outcome.message_id, Some(80));
    let requests = std::fs::read_to_string(directory.join("requests"))?;
    assert!(requests.contains("GET https://queue.example.com/messages?"));
    assert!(requests.contains("DELETE https://queue.example.com/messages/80"));
    Ok(())
}

fn session() -> TestResult<QueueSession> {
    struct Queue;
    impl Transport for Queue {
        fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
            assert_eq!(request.method, Method::Post);
            assert!(request.path.ends_with("/7/sessions"));
            Ok(Exchange {
                status: 200,
                body: br#"{"sessionId":"inbox-session","messageQueueUrl":"https://queue.example.com/messages","messageQueueAccessToken":"queue-token"}"#.to_vec(),
            })
        }
    }
    Ok(velnor_runner_github::create_session(
        &mut Queue,
        7,
        "inbox-owner",
        "admin",
    )?)
}
