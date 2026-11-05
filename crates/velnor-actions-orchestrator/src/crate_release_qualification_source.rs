// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError,
    crate_release_github::{GitHubAdmissionRead, verify_required_check},
    internal::internal,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedSourceRequest {
    pub repository: String,
    pub repository_id: u64,
    pub pull_request: u64,
    pub head_sha: String,
    pub default_branch: String,
    pub required_workflow_path: String,
    pub state: ReviewedPullState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewedPullState {
    OpenHead,
    Merged {
        merge_sha: String,
        merge_tree_sha: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundSourceEvidence {
    pub head_check_id: u64,
    pub merge_check_id: Option<u64>,
}

pub fn verify_pull_source(
    request: &ReviewedSourceRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<BoundSourceEvidence, OrchestratorError> {
    validate_request(request)?;
    let (first, evidence) = observe(request, api)?;
    let (second, fresh_evidence) = observe(request, api)?;
    if first != second || evidence != fresh_evidence {
        return Err(invalid());
    }
    Ok(evidence)
}

fn validate_request(r: &ReviewedSourceRequest) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(&r.repository)
        || !velnor_actions_contract::is_valid_branch_name(&r.default_branch)
        || r.repository_id == 0
        || r.pull_request == 0
        || !valid_sha(&r.head_sha)
        || !valid_workflow(&r.required_workflow_path)
    {
        return Err(invalid());
    }
    if let ReviewedPullState::Merged {
        merge_sha,
        merge_tree_sha,
    } = &r.state
        && (!valid_sha(merge_sha) || !valid_sha(merge_tree_sha))
    {
        return Err(invalid());
    }
    Ok(())
}

fn observe(
    r: &ReviewedSourceRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(Vec<Value>, BoundSourceEvidence), OrchestratorError> {
    let prefix = format!("repos/{}", r.repository);
    let repository = api.get(&prefix)?;
    let pull = api.get(&format!("{prefix}/pulls/{}", r.pull_request))?;
    let branch = encode(&r.default_branch);
    let mut observed = vec![repository, pull];
    let rules = read_rules(api, &prefix, &branch)?;
    validate_rules(&rules, matches!(r.state, ReviewedPullState::OpenHead))?;
    validate_pull(&observed[0], &observed[1], r)?;
    let head_check_id =
        verify_required_check(api, &r.repository, &r.head_sha, &r.required_workflow_path)?;
    let merge_check_id = if let ReviewedPullState::Merged {
        merge_sha,
        merge_tree_sha,
    } = &r.state
    {
        let commit = api.get(&format!("{prefix}/git/commits/{merge_sha}"))?;
        let compare = api.get(&format!("{prefix}/compare/{merge_sha}...{branch}"))?;
        if commit["sha"].as_str() != Some(merge_sha.as_str())
            || commit["tree"]["sha"].as_str() != Some(merge_tree_sha.as_str())
            || !matches!(compare["status"].as_str(), Some("ahead" | "identical"))
        {
            return Err(invalid());
        }
        observed.push(commit);
        observed.push(compare);
        Some(verify_required_check(
            api,
            &r.repository,
            merge_sha,
            &r.required_workflow_path,
        )?)
    } else {
        None
    };
    observed.extend(rules);
    Ok((
        observed,
        BoundSourceEvidence {
            head_check_id,
            merge_check_id,
        },
    ))
}

fn validate_pull(
    repository: &Value,
    pull: &Value,
    r: &ReviewedSourceRequest,
) -> Result<(), OrchestratorError> {
    if repository["id"].as_u64() != Some(r.repository_id)
        || repository["full_name"].as_str() != Some(r.repository.as_str())
        || repository["default_branch"].as_str() != Some(r.default_branch.as_str())
        || pull["number"].as_u64() != Some(r.pull_request)
        || pull["head"]["sha"].as_str() != Some(r.head_sha.as_str())
        || pull["head"]["repo"]["id"].as_u64() != Some(r.repository_id)
        || pull["head"]["repo"]["full_name"].as_str() != Some(r.repository.as_str())
        || pull["base"]["repo"]["id"].as_u64() != Some(r.repository_id)
        || pull["base"]["repo"]["full_name"].as_str() != Some(r.repository.as_str())
        || pull["base"]["ref"].as_str() != Some(r.default_branch.as_str())
    {
        return Err(invalid());
    }
    match &r.state {
        ReviewedPullState::OpenHead
            if pull["state"] == "open"
                && pull["draft"] == false
                && pull["merged"] == false
                && pull["mergeable_state"] == "clean" =>
        {
            Ok(())
        }
        ReviewedPullState::Merged { merge_sha, .. }
            if pull["state"] == "closed"
                && pull["merged"] == true
                && !pull["merged_at"].is_null()
                && pull["merge_commit_sha"].as_str() == Some(merge_sha.as_str()) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}

fn read_rules(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    branch: &str,
) -> Result<Vec<Value>, OrchestratorError> {
    let mut all = Vec::new();
    for page in 1..=100 {
        let value = api.get(&format!(
            "{prefix}/rules/branches/{branch}?per_page=100&page={page}"
        ))?;
        let rules = value.as_array().ok_or_else(invalid)?;
        if rules.len() > 100 || all.len() + rules.len() > 10_000 {
            return Err(invalid());
        }
        all.extend(rules.iter().cloned());
        if rules.len() < 100 {
            return Ok(all);
        }
    }
    Err(invalid())
}

fn validate_rules(rules: &[Value], strict: bool) -> Result<(), OrchestratorError> {
    let checks = rules.iter().any(|r| {
        r["type"] == "required_status_checks"
            && (!strict || r["parameters"]["strict_required_status_checks_policy"] == true)
            && r["parameters"]["required_status_checks"]
                .as_array()
                .is_some_and(|checks| checks.iter().any(|c| c["context"] == "Required"))
    });
    let reviews = rules.iter().any(|r| {
        r["type"] == "pull_request" && r["parameters"]["required_review_thread_resolution"] == true
    });
    if !checks || !reviews {
        return Err(invalid());
    }
    Ok(())
}
fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_workflow(value: &str) -> bool {
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
fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}
fn invalid() -> OrchestratorError {
    internal("qualification_reviewed_source_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_source.rs"]
mod tests;
