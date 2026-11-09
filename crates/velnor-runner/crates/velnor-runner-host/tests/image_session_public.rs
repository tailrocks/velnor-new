// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    dead_code,
    reason = "The peer also supports registry acquisition scenarios."
)]
#[path = "image_session_public/peer.rs"]
mod peer;

#[path = "image_session_public/stream.rs"]
mod stream;

use std::path::Path;
use std::{error::Error, io};

use peer::*;
use tempfile::TempDir;
use velnor_runner_host::ImageSession;

type TestResult<T> = Result<T, Box<dyn Error>>;

#[tokio::test]
async fn one_session_keeps_preloaded_pull_and_post_pull_checks_on_selected_peer() -> TestResult<()>
{
    let directory = TempDir::new()?;
    let socket = directory.path().join("docker.sock");
    let mut plan = Plan::default();
    plan.inspect
        .push_back(inspect_response(RUNNER_ID, RUNNER_DIGEST));
    plan.inspect.push_back(Response::bytes(404, Vec::new()));
    plan.inspect
        .push_back(inspect_response(PULLED_ID, MISSING_DIGEST));
    plan.pull.push_back(pull_response(MISSING_DIGEST));
    let mut peer_a = Peer::bind(&socket, plan)?;
    let endpoint = endpoint(&socket)?;
    let mut session = ImageSession::connect(endpoint, &selection()?).await?;
    let peer_b = replace_socket_with_peer_b(&directory, &socket, &mut peer_a)?;
    assert_preloaded_and_pulled_images(&mut session).await?;
    assert_selected_peer_received_anonymous_pull(&peer_a, &peer_b)?;
    drop(session);
    assert_later_operation_selects_replacement(endpoint, &peer_b).await?;
    drop(peer_a);
    drop(peer_b);
    Ok(())
}

fn replace_socket_with_peer_b(
    directory: &TempDir,
    socket: &Path,
    peer_a: &mut Peer,
) -> TestResult<Peer> {
    let moved = directory.path().join("docker-a.sock");
    std::fs::rename(socket, &moved)?;
    peer_a.moved_to(&moved);
    let mut plan = Plan::default();
    plan.inspect
        .push_back(inspect_response(RUNNER_ID, RUNNER_DIGEST));
    Peer::bind(socket, plan)
}

async fn assert_preloaded_and_pulled_images(session: &mut ImageSession) -> TestResult<()> {
    let preloaded = session
        .inspect(&image(RUNNER_DIGEST)?)
        .await?
        .ok_or_else(|| io::Error::other("preloaded image is absent"))?;
    observation_matches(&preloaded, RUNNER_ID, RUNNER_DIGEST);
    assert!(session.inspect(&image(MISSING_DIGEST)?).await?.is_none());
    let pulled = session.pull_and_inspect(&image(MISSING_DIGEST)?).await?;
    observation_matches(&pulled, PULLED_ID, MISSING_DIGEST);
    Ok(())
}

fn assert_selected_peer_received_anonymous_pull(peer_a: &Peer, peer_b: &Peer) -> TestResult<()> {
    let requests = peer_a.requests()?;
    let api_prefix = format!(
        "/v{}.{}",
        bollard::API_DEFAULT_VERSION.major_version,
        bollard::API_DEFAULT_VERSION.minor_version
    );
    assert!(
        requests
            .iter()
            .any(|request| request.path == format!("{api_prefix}/info"))
    );
    assert!(
        requests
            .iter()
            .all(|request| request.path.starts_with(&api_prefix))
    );
    assert!(requests.iter().any(|request| {
        request.method == "GET"
            && request.path.contains("/images/")
            && request.path.contains(
                "platform=%7B%22architecture%22%3A%22amd64%22%2C%22os%22%3A%22linux%22%7D",
            )
    }));
    let pull = requests
        .iter()
        .find(|request| request.method == "POST")
        .ok_or_else(|| io::Error::other("peer A did not receive digest pull"))?;
    assert!(
        pull.path
            .contains("fromImage=ghcr.io%2Ftailrocks%2Fvelnor-runner")
    );
    assert!(
        pull.path
            .contains(&format!("tag=sha256%3A{}", &MISSING_DIGEST[7..]))
    );
    assert!(pull.path.contains("platform=linux%2Famd64"));
    assert_eq!(pull.body.len(), 0);
    assert!(
        !pull
            .headers
            .iter()
            .any(|(name, _)| name == "authorization" || name == "x-registry-auth")
    );
    assert_eq!(peer_b.requests()?.len(), 0);
    Ok(())
}

async fn assert_later_operation_selects_replacement(
    endpoint: &str,
    peer_b: &Peer,
) -> TestResult<()> {
    let api_prefix = format!(
        "/v{}.{}",
        bollard::API_DEFAULT_VERSION.major_version,
        bollard::API_DEFAULT_VERSION.minor_version
    );
    let mut session = ImageSession::connect(endpoint, &selection()?).await?;
    let observed = session
        .inspect(&image(RUNNER_DIGEST)?)
        .await?
        .ok_or_else(|| io::Error::other("image is absent during later reinspection"))?;
    observation_matches(&observed, RUNNER_ID, RUNNER_DIGEST);
    assert!(
        peer_b
            .requests()?
            .iter()
            .any(|request| request.path == format!("{api_prefix}/info"))
    );
    Ok(())
}

fn endpoint(path: &Path) -> TestResult<&str> {
    path.to_str()
        .ok_or_else(|| io::Error::other("socket path is not UTF-8").into())
}

#[tokio::test]
async fn closed_selected_peer_never_reconnects_to_same_identity_replacement() -> TestResult<()> {
    let directory = TempDir::new()?;
    let socket = directory.path().join("docker.sock");
    let mut plan = Plan::default();
    plan.info.push_back(info_response(ENGINE_ID));
    plan.info.push_back(info_response(ENGINE_ID).close());
    let mut peer_a = Peer::bind(&socket, plan)?;
    let endpoint = endpoint(&socket)?;
    let mut session = ImageSession::connect(endpoint, &selection()?).await?;
    let moved = directory.path().join("docker-a.sock");
    std::fs::rename(&socket, &moved)?;
    peer_a.moved_to(&moved);
    let peer_b = Peer::bind(&socket, Plan::default())?;

    assert!(session.inspect(&image(RUNNER_DIGEST)?).await.is_err());
    assert_eq!(
        peer_a
            .requests()?
            .iter()
            .filter(|request| request.path.ends_with("/info"))
            .count(),
        2
    );
    assert_eq!(peer_b.requests()?.len(), 0);

    drop(session);
    drop(peer_a);
    drop(peer_b);
    Ok(())
}

#[tokio::test]
async fn closed_selected_sender_does_not_reconnect_after_send_failure() -> TestResult<()> {
    let directory = TempDir::new()?;
    let socket = directory.path().join("docker.sock");
    let mut plan = Plan::default();
    plan.info.push_back(info_response(ENGINE_ID).close());
    let mut peer_a = Peer::bind(&socket, plan)?;
    let endpoint = endpoint(&socket)?;
    let mut session = ImageSession::connect(endpoint, &selection()?).await?;
    let moved = directory.path().join("docker-a.sock");
    std::fs::rename(&socket, &moved)?;
    peer_a.moved_to(&moved);
    let mut plan = Plan::default();
    plan.info.push_back(info_response(ENGINE_ID));
    plan.inspect
        .push_back(inspect_response(RUNNER_ID, RUNNER_DIGEST));
    let peer_b = Peer::bind(&socket, plan)?;

    assert!(session.inspect(&image(RUNNER_DIGEST)?).await.is_err());
    assert_eq!(peer_a.requests()?.len(), 1);
    assert_eq!(peer_b.requests()?.len(), 0);

    drop(session);
    drop(peer_a);
    drop(peer_b);
    Ok(())
}

#[tokio::test]
async fn session_rejects_changed_selection_and_oversized_http_bodies() -> TestResult<()> {
    let directory = TempDir::new()?;
    for (name, response) in [
        ("engine", info_response("different-engine")),
        (
            "root",
            info_response_for(ENGINE_ID, "/var/lib/other-docker", "x86_64"),
        ),
        (
            "architecture",
            info_response_for(ENGINE_ID, ROOT, "aarch64"),
        ),
    ] {
        let socket = directory.path().join(format!("mismatch-{name}.sock"));
        let mut plan = Plan::default();
        plan.info.push_back(response);
        let _peer = Peer::bind(&socket, plan)?;
        let endpoint = endpoint(&socket)?;
        assert!(
            ImageSession::connect(endpoint, &selection()?)
                .await
                .is_err()
        );
    }

    let info_socket = directory.path().join("boundary-info.sock");
    let mut info_plan = Plan::default();
    info_plan
        .info
        .push_back(Response::padded_info(4 * 1024 * 1024));
    let info_peer = Peer::bind(&info_socket, info_plan)?;
    let info_endpoint = endpoint(&info_socket)?;
    let session = ImageSession::connect(info_endpoint, &selection()?).await?;
    drop(session);
    drop(info_peer);

    let oversized_socket = directory.path().join("oversized-valid-info.sock");
    let mut oversized_plan = Plan::default();
    oversized_plan
        .info
        .push_back(Response::padded_info(4 * 1024 * 1024 + 1));
    let oversized_peer = Peer::bind(&oversized_socket, oversized_plan)?;
    let oversized_endpoint = endpoint(&oversized_socket)?;
    assert!(
        ImageSession::connect(oversized_endpoint, &selection()?)
            .await
            .is_err()
    );
    drop(oversized_peer);

    let pull_socket = directory.path().join("oversized-pull.sock");
    let mut pull_plan = Plan::default();
    pull_plan
        .inspect
        .push_back(Response::bytes(404, Vec::new()));
    pull_plan.pull.push_back(Response::oversized());
    let pull_peer = Peer::bind(&pull_socket, pull_plan)?;
    let pull_endpoint = endpoint(&pull_socket)?;
    let mut session = ImageSession::connect(pull_endpoint, &selection()?).await?;
    assert!(session.inspect(&image(MISSING_DIGEST)?).await?.is_none());
    assert!(
        session
            .pull_and_inspect(&image(MISSING_DIGEST)?)
            .await
            .is_err()
    );
    drop(session);
    drop(pull_peer);
    Ok(())
}

#[tokio::test]
async fn session_rejects_identity_change_during_inspection_recheck() -> TestResult<()> {
    let directory = TempDir::new()?;
    let socket = directory.path().join("docker.sock");
    let mut plan = Plan::default();
    plan.info.push_back(info_response(ENGINE_ID));
    plan.info.push_back(info_response(ENGINE_ID));
    plan.info.push_back(info_response("changed-engine"));
    plan.inspect
        .push_back(inspect_response(RUNNER_ID, RUNNER_DIGEST));
    let peer = Peer::bind(&socket, plan)?;
    let endpoint = endpoint(&socket)?;
    let mut session = ImageSession::connect(endpoint, &selection()?).await?;
    assert!(session.inspect(&image(RUNNER_DIGEST)?).await.is_err());
    drop(session);
    drop(peer);
    Ok(())
}

#[tokio::test]
async fn digest_pull_rejects_missing_wrong_and_malformed_terminal_evidence() -> TestResult<()> {
    for body in [
        b"{\"status\":\"Pulling\"}\n".as_slice(),
        b"{\"status\":\"Digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}\n",
        b"{\"status\":\"truncated",
        b"{\"status\":\"one\",\"status\":\"two\"}\n",
    ] {
        let directory = TempDir::new()?;
        let socket = directory.path().join("docker.sock");
        let mut plan = Plan::default();
        plan.pull.push_back(Response::bytes(200, body.to_vec()));
        plan.inspect.push_back(inspect_response(PULLED_ID, MISSING_DIGEST));
        let peer = Peer::bind(&socket, plan)?;
        let mut session = ImageSession::connect(endpoint(&socket)?, &selection()?).await?;
        assert!(session.pull_and_inspect(&image(MISSING_DIGEST)?).await.is_err());
        let requests = peer.requests()?;
        assert!(requests.iter().any(|request| request.method == "POST" && request.path.contains(&format!("tag=sha256%3A{}", &MISSING_DIGEST[7..]))));
        assert!(!requests.iter().any(|request| request.method == "GET" && request.path.contains("/images/")));
        drop(session);
        drop(peer);
    }
    Ok(())
}
