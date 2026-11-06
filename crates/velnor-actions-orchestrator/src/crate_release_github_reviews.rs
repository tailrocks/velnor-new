// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, Value, internal,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ReviewAuthor<'a> {
    pub(super) login: &'a str,
    pub(super) id: u64,
}

struct ReviewDecision {
    state: String,
    commit: String,
    submitted: String,
    human: bool,
}

struct ReviewPermission {
    human: bool,
    allowed: bool,
}

struct ReviewActor<'a> {
    login: &'a str,
    id: u64,
    human: bool,
    association: &'a str,
}

impl<'a> ReviewActor<'a> {
    fn read(review: &'a Value) -> Result<Self, OrchestratorError> {
        Ok(Self {
            login: review["user"]["login"]
                .as_str()
                .ok_or_else(|| internal("crate_release_review_user"))?,
            id: review["user"]["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("crate_release_review_user_id"))?,
            human: match review["user"]["type"].as_str() {
                Some("User") => true,
                Some("Bot") => false,
                _ => return Err(internal("crate_release_review_user_type")),
            },
            association: review["author_association"].as_str().unwrap_or_default(),
        })
    }

    fn authorized(
        &self,
        api: &dyn GitHubAdmissionRead,
        prefix: &str,
        author: ReviewAuthor<'_>,
        permissions: &mut BTreeMap<u64, ReviewPermission>,
    ) -> Result<bool, OrchestratorError> {
        if self.id == author.id
            || self.login.eq_ignore_ascii_case(author.login)
            || !matches!(self.association, "OWNER" | "MEMBER" | "COLLABORATOR")
        {
            return Ok(false);
        }
        if let Some(permission) = permissions.get(&self.id) {
            require(permission.human == self.human, "review_permission_identity")?;
            return Ok(permission.allowed);
        }
        let permission = api.get(&format!(
            "{prefix}/collaborators/{}/permission",
            encode(self.login)
        ))?;
        require(
            permission["user"]["id"].as_u64() == Some(self.id)
                && permission["user"]["type"].as_str()
                    == Some(if self.human { "User" } else { "Bot" }),
            "review_permission_identity",
        )?;
        let allowed = matches!(permission["permission"].as_str(), Some("write" | "admin"));
        permissions.insert(
            self.id,
            ReviewPermission {
                human: self.human,
                allowed,
            },
        );
        Ok(allowed)
    }
}

pub(super) fn verify_reviews(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    author: ReviewAuthor<'_>,
    head: &str,
    merged: &str,
) -> Result<(), OrchestratorError> {
    let latest = collect_review_decisions(api, prefix, number, author, merged)?;
    require(
        !latest
            .values()
            .any(|decision| decision.state == "CHANGES_REQUESTED"),
        "release_pr_changes_requested",
    )?;
    require(
        latest.values().any(|decision| {
            decision.human && decision.state == "APPROVED" && decision.commit == head
        }),
        "release_pr_approval",
    )
}

fn read_decision(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    review: &Value,
    author: ReviewAuthor<'_>,
    merged: &str,
    permissions: &mut BTreeMap<u64, ReviewPermission>,
) -> Result<Option<(u64, ReviewDecision)>, OrchestratorError> {
    let state = review["state"].as_str().unwrap_or_default();
    if state == "PENDING" || state == "COMMENTED" {
        return Ok(None);
    }
    let actor = ReviewActor::read(review)?;
    if !actor.authorized(api, prefix, author, permissions)? {
        return Ok(None);
    }
    require(
        matches!(state, "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"),
        "review_state",
    )?;
    let submitted = review["submitted_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_review_time"))?;
    if submitted > merged {
        return Ok(None);
    }
    Ok(Some((
        actor.id,
        ReviewDecision {
            state: state.to_owned(),
            commit: review["commit_id"].as_str().unwrap_or_default().to_owned(),
            submitted: submitted.to_owned(),
            human: actor.human,
        },
    )))
}

fn collect_review_decisions(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    number: u64,
    author: ReviewAuthor<'_>,
    merged: &str,
) -> Result<BTreeMap<u64, ReviewDecision>, OrchestratorError> {
    let mut latest: BTreeMap<u64, ReviewDecision> = BTreeMap::new();
    let mut ambiguous = std::collections::BTreeSet::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut permissions = BTreeMap::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/pulls/{number}/reviews?per_page=100&page={page}"
        ))?;
        let reviews = response
            .as_array()
            .filter(|items| items.len() <= 100)
            .ok_or_else(|| internal("crate_release_reviews_invalid"))?;
        let count = reviews.len();
        for review in reviews {
            let id = review["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| internal("crate_release_review_id"))?;
            require(seen_ids.insert(id), "review_id_duplicate")?;
            let Some((actor, decision)) =
                read_decision(api, prefix, review, author, merged, &mut permissions)?
            else {
                continue;
            };
            if let Some(previous) = latest.get(&actor) {
                if previous.submitted == decision.submitted {
                    ambiguous.insert(actor);
                    continue;
                }
                if previous.submitted > decision.submitted {
                    continue;
                }
            }
            ambiguous.remove(&actor);
            latest.insert(actor, decision);
        }
        if count < 100 {
            break;
        }
        require(page < 100, "reviews_limit")?;
    }
    require(ambiguous.is_empty(), "review_order_ambiguous")?;
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
