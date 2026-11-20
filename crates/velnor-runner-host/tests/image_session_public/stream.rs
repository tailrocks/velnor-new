// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[tokio::test]
async fn pull_accepts_large_progress_stream_and_inspects_image() -> TestResult<()> {
    let directory = TempDir::new()?;
    let socket = directory.path().join("docker.sock");
    let mut body = format!("{{\"status\":\"{}\"}}\n", "x".repeat(1024))
        .repeat(5000)
        .into_bytes();
    body.extend_from_slice(&pull_response(MISSING_DIGEST).body_bytes());
    let mut plan = Plan::default();
    plan.pull.push_back(Response::bytes(200, body));
    plan.inspect
        .push_back(inspect_response(PULLED_ID, MISSING_DIGEST));
    let peer = Peer::bind(&socket, plan)?;
    let mut session = ImageSession::connect(endpoint(&socket)?, &selection()?).await?;
    let observed = session.pull_and_inspect(&image(MISSING_DIGEST)?).await?;
    observation_matches(&observed, PULLED_ID, MISSING_DIGEST);
    drop(session);
    drop(peer);
    Ok(())
}

#[tokio::test]
async fn pull_rejects_oversized_record_excess_messages_and_daemon_error() -> TestResult<()> {
    let cases = [
        format!("{{\"status\":\"{}\"}}\n", "x".repeat(64 * 1024)),
        "{\"status\":\"progress\"}\n".repeat(16_385),
        "{\"status\":\"progress\",\"error\":\"failed\"}\n".to_owned(),
    ];
    for mut body in cases {
        body.push_str(std::str::from_utf8(
            &pull_response(MISSING_DIGEST).body_bytes(),
        )?);
        let directory = TempDir::new()?;
        let socket = directory.path().join("docker.sock");
        let mut plan = Plan::default();
        plan.pull.push_back(Response::bytes(200, body.into_bytes()));
        let peer = Peer::bind(&socket, plan)?;
        let mut session = ImageSession::connect(endpoint(&socket)?, &selection()?).await?;
        assert!(
            session
                .pull_and_inspect(&image(MISSING_DIGEST)?)
                .await
                .is_err()
        );
        assert!(
            !peer
                .requests()?
                .iter()
                .any(|request| request.path.contains("/images/") && request.method == "GET")
        );
        drop(session);
        drop(peer);
    }
    Ok(())
}
