// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::time::{Duration, Instant};

use velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
use velnor_actions_orchestrator::{
    BunAssetsRepositoryRead, OrchestratorError, check_bun_assets_repository,
};

const REPOSITORY: &str = "fixture-owner/consumer";
const TOKEN: &str = "workflow-token-fixture";
const PRIVATE: &str = r#"{"full_name":"fixture-owner/consumer","private":true,"visibility":"private","owner":{"login":"fixture-owner"}}"#;
const PUBLIC: &str =
    r#"{"full_name":"fixture-owner/consumer","private":false,"visibility":"public"}"#;

const REJECTED_METADATA: [(&str, &str); 13] = [
    ("public", PUBLIC),
    (
        "internal",
        r#"{"full_name":"fixture-owner/consumer","private":true,"visibility":"internal"}"#,
    ),
    (
        "missing private",
        r#"{"full_name":"fixture-owner/consumer","visibility":"private"}"#,
    ),
    (
        "null private",
        r#"{"full_name":"fixture-owner/consumer","private":null,"visibility":"private"}"#,
    ),
    (
        "string private",
        r#"{"full_name":"fixture-owner/consumer","private":"true","visibility":"private"}"#,
    ),
    (
        "conflicting visibility",
        r#"{"full_name":"fixture-owner/consumer","private":true,"visibility":"public"}"#,
    ),
    (
        "missing visibility",
        r#"{"full_name":"fixture-owner/consumer","private":true}"#,
    ),
    (
        "duplicate private",
        r#"{"full_name":"fixture-owner/consumer","private":false,"private":true,"visibility":"private"}"#,
    ),
    (
        "escaped duplicate",
        r#"{"full_name":"fixture-owner/consumer","private":false,"pr\u0069vate":true,"visibility":"private"}"#,
    ),
    (
        "nested duplicate",
        r#"{"full_name":"fixture-owner/consumer","private":true,"visibility":"private","owner":{"id":1,"id":2}}"#,
    ),
    (
        "wrong identity",
        r#"{"full_name":"fixture-owner/other","private":true,"visibility":"private"}"#,
    ),
    (
        "missing identity",
        r#"{"private":true,"visibility":"private"}"#,
    ),
    ("malformed", "{not json"),
];

struct GitHubMetadataResponses {
    responses: RefCell<VecDeque<Result<Vec<u8>, String>>>,
}

impl GitHubMetadataResponses {
    fn json(responses: &[&str]) -> Self {
        Self {
            responses: RefCell::new(
                responses
                    .iter()
                    .map(|body| Ok(body.as_bytes().to_vec()))
                    .collect(),
            ),
        }
    }
}

impl BunAssetsRepositoryRead for GitHubMetadataResponses {
    fn repository_json(
        &self,
        _repository: &str,
        _token: &str,
        _deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err("unexpected external metadata request".to_owned()))
            .map_err(|problem| OrchestratorError::Internal { problem })
    }
}

fn deadline() -> Result<Instant, Box<dyn Error>> {
    Instant::now()
        .checked_add(Duration::from_secs(30))
        .ok_or_else(|| "deadline overflow".into())
}

#[test]
fn private_asset_repository_admission_rejects_unverifiable_visibility() -> Result<(), Box<dyn Error>>
{
    for (label, body) in REJECTED_METADATA {
        let read = GitHubMetadataResponses::json(&[body]);
        assert!(
            check_bun_assets_repository(&read, REPOSITORY, TOKEN, deadline()?).is_err(),
            "accepted {label} repository metadata"
        );
    }
    let read = GitHubMetadataResponses {
        responses: RefCell::new(VecDeque::from([Ok(vec![
            b' ';
            MAX_UNTRUSTED_DOCUMENT_BYTES + 1
        ])])),
    };
    let error = check_bun_assets_repository(&read, REPOSITORY, TOKEN, deadline()?)
        .err()
        .ok_or("oversized metadata must fail")?;
    assert!(
        error
            .to_string()
            .contains("bun_assets_repository_metadata_invalid")
    );
    Ok(())
}

#[test]
fn private_asset_repository_visibility_is_checked_again_before_upload() -> Result<(), Box<dyn Error>>
{
    let read = GitHubMetadataResponses::json(&[PRIVATE, PUBLIC]);
    check_bun_assets_repository(&read, REPOSITORY, TOKEN, deadline()?)?;
    let error = check_bun_assets_repository(&read, REPOSITORY, TOKEN, deadline()?)
        .err()
        .ok_or("public transition must reject upload")?;
    assert!(
        error
            .to_string()
            .contains("bun_assets_repository_not_private")
    );
    let read = GitHubMetadataResponses::json(&[PRIVATE]);
    check_bun_assets_repository(&read, "Fixture-Owner/Consumer", TOKEN, deadline()?)?;
    Ok(())
}

#[test]
fn private_asset_repository_transport_and_deadline_fail_closed() -> Result<(), Box<dyn Error>> {
    let read = GitHubMetadataResponses {
        responses: RefCell::new(VecDeque::from([Err("external GitHub HTTP 503".to_owned())])),
    };
    let error = check_bun_assets_repository(&read, REPOSITORY, TOKEN, deadline()?)
        .err()
        .ok_or("metadata HTTP error must fail")?;
    assert!(error.to_string().contains("external GitHub HTTP 503"));
    let read = GitHubMetadataResponses::json(&[PRIVATE]);
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .ok_or("deadline underflow")?;
    let error = check_bun_assets_repository(&read, REPOSITORY, TOKEN, expired)
        .err()
        .ok_or("expired metadata deadline must fail")?;
    assert!(
        error
            .to_string()
            .contains("bun_assets_repository_deadline_exceeded")
    );
    assert_eq!(
        read.responses.borrow().len(),
        1,
        "expired request must not fetch metadata"
    );
    Ok(())
}
