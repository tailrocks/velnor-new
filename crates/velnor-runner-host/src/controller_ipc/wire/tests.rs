// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write;
use std::os::unix::net::UnixStream;

use super::{MAX_REQUEST_BYTES, Request, Target};
use crate::config::{
    ConnectionId, KeychainReference, PullRequestRule, RepositoryConnection, ScaleSetSelector,
    SourcePolicy, SourcePolicyMode, WorkflowEvent,
};
use crate::controller_ipc::server::read_request_for_test;
use crate::ipc::{MAX_FRAME, encode_frame};

type TestResult = Result<(), Box<dyn Error>>;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// The largest binding `SourcePolicy::validate` accepts: 128 pull-request
/// rules with 255-character branches.
fn largest_binding() -> Result<RepositoryConnection, Box<dyn Error>> {
    let rules = (0..128_u64)
        .map(|index| PullRequestRule {
            number: Some(index + 1),
            base_repository_id: 42,
            base_branch: format!("{index:03}{}", "b".repeat(252)),
            head_repository_id: 43,
            head_branch: format!("{index:03}{}", "h".repeat(252)),
        })
        .collect();
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("lab")?,
        42,
        "tailrocks/velnor-lab".to_owned(),
        KeychainReference::parse("keychain:com.tailrocks.velnor.host/lab")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-arm-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-arm-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            rules,
        )?,
    )?)
}

fn binding_requests() -> Result<Vec<Request>, Box<dyn Error>> {
    let binding = largest_binding()?;
    Ok(vec![
        Request::DrainStatus {
            target: Target::Connection {
                connection_id: binding.connection_id.clone(),
                binding: Box::new(binding.clone()),
            },
            token: TOKEN.to_owned(),
        },
        Request::Reactivate {
            binding: binding.clone(),
            token: TOKEN.to_owned(),
        },
        Request::Detached {
            binding,
            token: TOKEN.to_owned(),
        },
    ])
}

#[test]
fn a_legal_largest_binding_is_over_the_old_64_kib_cap() -> TestResult {
    let bytes = serde_json::to_vec(&largest_binding()?)?;
    assert!(bytes.len() > 65_536, "binding is {} bytes", bytes.len());
    assert!(bytes.len() < MAX_FRAME);
    Ok(())
}

#[test]
fn drain_reactivate_and_detached_encode_a_legal_largest_binding() -> TestResult {
    for request in binding_requests()? {
        let bytes = request
            .encode()
            .map_err(|()| "encode refused the request")?;
        assert!(bytes.len() > 65_536);
    }
    Ok(())
}

#[test]
fn the_server_accepts_a_framed_largest_binding_request() -> TestResult {
    for request in binding_requests()? {
        let bytes = request.encode().map_err(|()| "encode refused")?;
        let (mut client, mut server) = UnixStream::pair()?;
        let frame = encode_frame(&bytes)?;
        let writer = std::thread::spawn(move || client.write_all(&frame));
        let decoded = read_request_for_test(&mut server)?;
        writer.join().map_err(|_| "writer panicked")??;
        assert_eq!(serde_json::to_vec(&decoded)?, bytes);
    }
    Ok(())
}

#[test]
fn a_body_over_the_request_cap_is_still_refused() {
    let request = Request::ControllerReadiness {
        config_sha256: "a".repeat(MAX_REQUEST_BYTES + 1),
        engine_id: String::new(),
        docker_root_digest: String::new(),
    };
    assert!(request.encode().is_err());
}
