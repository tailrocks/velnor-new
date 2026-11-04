// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BTreeMap, GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, internal};

struct ReviewDecision {
    id: u64,
    user_id: u64,
    state: String,
    commit: String,
    association: String,
    submitted: String,
    human: bool,
}

pub(super) fn verify_reviews(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    author: &str,
    head: &str,
    merged: &str,
) -> Result<(), OrchestratorError> {
    let latest = collect_review_decisions(api, prefix, number, merged)?;
    require(
        !latest
            .values()
            .any(|decision| decision.state == "CHANGES_REQUESTED"),
        "release_pr_changes_requested",
    )?;
    for (login, decision) in latest {
        if !decision.human
            || login.eq_ignore_ascii_case(author)
            || decision.state != "APPROVED"
            || decision.commit != head
            || !matches!(
                decision.association.as_str(),
                "OWNER" | "MEMBER" | "COLLABORATOR"
            )
        {
            continue;
        }
        let permission = api.get(&format!(
            "{prefix}/collaborators/{}/permission",
            encode(&login)
        ))?;
        if permission["user"]["id"].as_u64() == Some(decision.user_id)
            && permission["user"]["type"].as_str() == Some("User")
            && matches!(permission["permission"].as_str(), Some("write" | "admin"))
        {
            return Ok(());
        }
    }
    Err(internal("crate_release_release_pr_approval"))
}

fn collect_review_decisions(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    merged: &str,
) -> Result<BTreeMap<String, ReviewDecision>, OrchestratorError> {
    let mut latest: BTreeMap<String, ReviewDecision> = BTreeMap::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/pulls/{number}/reviews?per_page=100&page={page}"
        ))?;
        let reviews = response
            .as_array()
            .ok_or_else(|| internal("crate_release_reviews_invalid"))?;
        let count = reviews.len();
        for review in reviews {
            let Some(login) = review["user"]["login"].as_str() else {
                return Err(internal("crate_release_review_user"));
            };
            let human = match review["user"]["type"].as_str() {
                Some("User") => true,
                Some("Bot") => false,
                _ => return Err(internal("crate_release_review_user_type")),
            };
            let user_id = review["user"]["id"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("crate_release_review_user_id"))?;
            let association = review["author_association"].as_str().unwrap_or_default();
            let id = review["id"]
                .as_u64()
                .ok_or_else(|| internal("crate_release_review_id"))?;
            let state = review["state"].as_str().unwrap_or_default();
            if state == "PENDING" || state == "COMMENTED" {
                continue;
            }
            let submitted = review["submitted_at"]
                .as_str()
                .filter(|value| valid_utc(value))
                .ok_or_else(|| internal("crate_release_review_time"))?;
            if submitted >= merged {
                continue;
            }
            let commit = review["commit_id"].as_str().unwrap_or_default();
            let login = login.to_ascii_lowercase();
            if let Some(previous) = latest.get(&login) {
                if previous.submitted == submitted {
                    require(previous.id == id, "review_order_ambiguous")?;
                    continue;
                }
                if previous.submitted.as_str() > submitted {
                    continue;
                }
            }
            latest.insert(
                login,
                ReviewDecision {
                    id,
                    user_id,
                    state: state.to_owned(),
                    commit: commit.to_owned(),
                    association: association.to_owned(),
                    submitted: submitted.to_owned(),
                    human,
                },
            );
        }
        if count < 100 {
            break;
        }
        require(page < 100, "reviews_limit")?;
    }
    Ok(latest)
}

pub(super) fn validate_request(request: &GitHubAdmissionRequest) -> Result<(), OrchestratorError> {
    let Some((owner, repo)) = request.repository.split_once('/') else {
        return Err(internal("crate_release_repository"));
    };
    require(
        velnor_actions_contract::config::is_valid_github_owner(owner)
            && velnor_actions_contract::config::is_valid_github_repo_name(repo),
        "repository",
    )?;
    require(
        !request.default_branch.is_empty()
            && request.default_branch.len() <= 255
            && !request.default_branch.contains(['?', '#', '\\']),
        "default_branch",
    )?;
    require(
        !request.environment.is_empty()
            && request.environment.len() <= 255
            && !request.environment.contains(['?', '#', '\\']),
        "environment",
    )?;
    require(
        valid_workflow_path(&request.workflow_path)
            && valid_workflow_path(&request.required_workflow_path),
        "workflow_path",
    )?;
    require(
        valid_sha(&request.workflow_sha) && valid_sha(&request.source_sha),
        "sha",
    )?;
    require(
        request.run_id > 0 && request.run_attempt > 0 && request.trusted_reviewer_id > 0,
        "run",
    )?;
    require(
        matches!(request.event.as_str(), "push" | "workflow_dispatch"),
        "event",
    )?;
    Ok(())
}

pub(super) fn valid_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

pub(super) fn valid_workflow_path(value: &str) -> bool {
    value
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| {
            name.rsplit_once('.')
                .is_some_and(|(_, ext)| matches!(ext, "yml" | "yaml"))
                && !name.starts_with('.')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

pub(super) fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn require(value: bool, reason: &str) -> Result<(), OrchestratorError> {
    if value {
        Ok(())
    } else {
        Err(internal(&format!("crate_release_{reason}")))
    }
}

pub(super) fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    out
}
