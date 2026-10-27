// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use velnor_actions_contract::strict_json::{MAX_UNTRUSTED_DOCUMENT_BYTES, parse_strict_json_bytes};

use super::github_evidence_http::{
    EvidenceRoute, GitHubEvidenceRead, GitHubRepository, NativeGitHubEvidenceRead,
};
use crate::OrchestratorError;
use crate::internal::internal;

pub const BUN_ASSETS_REPOSITORY_CHECK_OP: &str =
    velnor_actions_contract::BUN_ASSETS_REPOSITORY_CHECK_OPERATION;

pub trait BunAssetsRepositoryRead {
    fn repository_json(
        &self,
        repository: &str,
        token: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError>;
}

struct NativeRepositoryRead;

impl BunAssetsRepositoryRead for NativeRepositoryRead {
    fn repository_json(
        &self,
        repository: &str,
        token: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        let repository = GitHubRepository::parse(repository)?;
        NativeGitHubEvidenceRead.json(&repository, &EvidenceRoute::Repository, token, deadline)
    }
}

pub fn check_bun_assets_repository(
    read: &dyn BunAssetsRepositoryRead,
    repository: &str,
    token: &str,
    deadline: Instant,
) -> Result<(), OrchestratorError> {
    GitHubRepository::parse(repository)?;
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(internal("bun_assets_repository_token_invalid"));
    }
    check_deadline(deadline)?;
    let bytes = read.repository_json(repository, token, deadline)?;
    check_deadline(deadline)?;
    let metadata = parse_strict_json_bytes(&bytes, MAX_UNTRUSTED_DOCUMENT_BYTES)
        .map_err(|_| internal("bun_assets_repository_metadata_invalid"))?;
    if !metadata
        .get("full_name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(repository))
    {
        return Err(internal("bun_assets_repository_identity_mismatch"));
    }
    if metadata.get("private").and_then(serde_json::Value::as_bool) != Some(true)
        || metadata
            .get("visibility")
            .and_then(serde_json::Value::as_str)
            != Some("private")
    {
        return Err(internal("bun_assets_repository_not_private"));
    }
    check_deadline(deadline)
}

pub fn run_bun_assets_repository_check() -> Result<(), OrchestratorError> {
    let repository = std::env::var("VELNOR_BUN_ASSET_REPOSITORY")
        .map_err(|_| internal("bun_assets_repository_identity_missing"))?;
    let destination = std::env::var("GITHUB_REPOSITORY")
        .map_err(|_| internal("bun_assets_repository_destination_missing"))?;
    if !repository.eq_ignore_ascii_case(&destination) {
        return Err(internal("bun_assets_repository_destination_mismatch"));
    }
    let token =
        std::env::var("GH_TOKEN").map_err(|_| internal("bun_assets_repository_token_missing"))?;
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(30))
        .ok_or_else(|| internal("bun_assets_repository_deadline_invalid"))?;
    check_bun_assets_repository(&NativeRepositoryRead, &repository, &token, deadline)
}

fn check_deadline(deadline: Instant) -> Result<(), OrchestratorError> {
    if Instant::now() >= deadline {
        Err(internal("bun_assets_repository_deadline_exceeded"))
    } else {
        Ok(())
    }
}
