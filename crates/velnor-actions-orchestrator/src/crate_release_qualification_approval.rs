// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError, crate_release_github::GitHubAdmissionRead,
    crate_release_qualification_witness::WitnessRequest, internal::internal,
};
use serde_json::Value;

pub fn verify_rehearsal_approval(
    request: &WitnessRequest,
    default_branch: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(&request.repository)
        || !velnor_actions_contract::is_valid_branch_name(default_branch)
        || request.environment.is_empty()
        || !request
            .environment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || request.workflow_sha.len() != 40
        || !request
            .workflow_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || request.repository_id == 0
        || request.run_id == 0
        || request.run_attempt != 1
        || request.reviewer_id == 0
        || request.workflow_path != ".github/workflows/release-rehearsal.yml"
    {
        return Err(invalid());
    }
    let prefix = format!("repos/{}", request.repository);
    let paths = [
        prefix.clone(),
        format!("{prefix}/branches/{}", encode(default_branch)),
        format!("{prefix}/actions/runs/{}/attempts/1", request.run_id),
        format!("{prefix}/environments/{}", encode(&request.environment)),
        format!("{prefix}/actions/runs/{}/approvals", request.run_id),
    ];
    let observed = paths
        .iter()
        .map(|path| api.get(path))
        .collect::<Result<Vec<_>, _>>()?;
    validate(&observed, request, default_branch)?;
    let again = paths
        .iter()
        .map(|path| api.get(path))
        .collect::<Result<Vec<_>, _>>()?;
    if observed != again {
        return Err(invalid());
    }
    Ok(())
}

fn validate(values: &[Value], r: &WitnessRequest, branch: &str) -> Result<(), OrchestratorError> {
    let [repository, head, run, environment, approvals] = values else {
        return Err(invalid());
    };
    if repository["id"].as_u64() != Some(r.repository_id)
        || repository["full_name"].as_str() != Some(r.repository.as_str())
        || repository["default_branch"].as_str() != Some(branch)
        || head["name"].as_str() != Some(branch)
        || head["protected"] != true
        || head["commit"]["sha"].as_str() != Some(r.workflow_sha.as_str())
        || run["id"].as_u64() != Some(r.run_id)
        || run["run_attempt"].as_u64() != Some(1)
        || run["event"] != "workflow_dispatch"
        || run["head_sha"].as_str() != Some(r.workflow_sha.as_str())
        || run["head_branch"].as_str() != Some(branch)
        || run["path"].as_str() != Some(r.workflow_path.as_str())
        || run["repository"]["id"].as_u64() != Some(r.repository_id)
        || run["head_repository"]["id"].as_u64() != Some(r.repository_id)
        || run["status"] != "completed"
        || run["conclusion"] != "success"
        || environment["name"].as_str() != Some(r.environment.as_str())
        || environment["can_admins_bypass"] != false
        || environment["deployment_branch_policy"]["protected_branches"] != true
        || environment["deployment_branch_policy"]["custom_branch_policies"] != false
    {
        return Err(invalid());
    }
    let id = environment["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    let rules = environment["protection_rules"]
        .as_array()
        .ok_or_else(invalid)?;
    let reviewers: Vec<&Value> = rules
        .iter()
        .filter(|rule| rule["type"] == "required_reviewers")
        .collect();
    if reviewers.len() != 1 {
        return Err(invalid());
    }
    let allowed = reviewers[0]["reviewers"].as_array().ok_or_else(invalid)?;
    let matching = allowed
        .iter()
        .filter(|entry| {
            entry["type"] == "User"
                && entry["reviewer"]["type"] == "User"
                && entry["reviewer"]["id"].as_u64() == Some(r.reviewer_id)
        })
        .count();
    if matching != 1 {
        return Err(invalid());
    }
    let approvals = approvals.as_array().ok_or_else(invalid)?;
    let approval = crate::crate_release_github::trusted_environment_approval(
        approvals,
        id,
        &r.environment,
        r.reviewer_id,
    )
    .ok_or_else(invalid)?;
    if approval["state"] != "approved" || approval["user"]["type"] != "User" {
        return Err(invalid());
    }
    Ok(())
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
    internal("qualification_approval_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_approval.rs"]
mod tests;
