// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
pub(super) fn test_metadata_responses(sha: &str) -> (serde_json::Value, serde_json::Value) {
    (
        serde_json::json!({"ref":"refs/tags/demo-v0.1.1","object":{"type":"commit","sha":sha}}),
        serde_json::json!({"id":12,"tag_name":"demo-v0.1.1","draft":false,"prerelease":false,"html_url":"https://github.com/owner/project/releases/tag/demo-v0.1.1"}),
    )
}

#[cfg(unix)]
pub(super) fn test_changing_workflow_response(
    line: &str,
    previous: &[String],
) -> Option<serde_json::Value> {
    let sha = "d".repeat(40);
    if !line.contains(&sha) {
        return None;
    }
    if line.contains("/check-runs?") {
        return Some(
            serde_json::json!({"total_count":1,"check_runs":[{"id":31,"name":"Required","head_sha":sha,"check_suite":{"id":131},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]}),
        );
    }
    if line.contains("/actions/runs?") {
        let mut runs = vec![
            serde_json::json!({"check_suite_id":131,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}),
        ];
        if previous.iter().any(|entry| entry == line) {
            runs.push(serde_json::json!({"check_suite_id":133,"head_sha":sha,"path":".github/workflows/ci.yml","status":"in_progress","conclusion":null}));
        }
        return Some(serde_json::json!({"total_count":runs.len(),"workflow_runs":runs}));
    }
    None
}

#[cfg(unix)]
pub(in super::super) fn test_github_admission_response(
    line: &str,
    policy: &str,
    source: &str,
) -> Option<serde_json::Value> {
    let special_sha = ["e", "f", "9"]
        .into_iter()
        .map(|value| value.repeat(40))
        .find(|sha| line.contains(sha));
    let value = if let Some(sha) = special_sha
        .as_deref()
        .filter(|_| line.contains("/check-runs?"))
    {
        let failed = sha.starts_with('f');
        serde_json::json!({"total_count":2,"check_runs":[
            {"id":31,"name":"Required","head_sha":sha,"check_suite":{"id":131},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":if failed {"failure"} else {"success"},"started_at":"2026-11-03T15:40:00Z"},
            {"id":32,"name":"Required","head_sha":sha,"check_suite":{"id":132},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:41:00Z"}
        ]})
    } else if let Some(sha) = special_sha
        .as_deref()
        .filter(|_| line.contains("/actions/runs?"))
    {
        let event = if sha.starts_with('9') {
            "pull_request"
        } else {
            "push"
        };
        serde_json::json!({"total_count":2,"workflow_runs":[
            {"check_suite_id":131,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success","event":event,"head_branch":"main","repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}},"head_repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}}},
            {"check_suite_id":132,"head_sha":sha,"path":".github/workflows/other.yml","status":"completed","conclusion":"success"}
        ]})
    } else if line == "GET /repos/owner/project/git/ref/heads/main HTTP/1.1" {
        serde_json::json!({"ref":"refs/heads/main","object":{"type":"commit","sha":policy}})
    } else if line.starts_with("GET /repos/owner/project/actions/runs/13/approvals ") {
        serde_json::json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy} and source {policy}, and authorize this source for run 13.")}])
    } else if line.starts_with("GET /repos/owner/project/actions/runs/13 ") {
        serde_json::json!({"id":13,"run_attempt":1,"head_sha":policy,"head_branch":"main","event":"push","path":".github/workflows/crate-release.yml","repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}},"head_repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}}})
    } else if line.contains("/pulls?") {
        serde_json::json!([{"number":42,"merged_at":"2026-11-03T16:00:00Z","merge_commit_sha":policy,"base":{"ref":"main","repo":{"full_name":"owner/project","owner":{"id":77,"login":"owner"}}},"head":{"ref":"release-plz-fixture","sha":"c".repeat(40),"repo":{"full_name":"owner/project","owner":{"id":77,"login":"owner"}}},"user":{"id":54,"login":"release-bot"}}])
    } else if line.contains("/git/commits/") {
        let sha = if line.contains(policy) {
            policy
        } else {
            "cccccccccccccccccccccccccccccccccccccccc"
        };
        serde_json::json!({"sha":sha,"tree":{"sha":"d".repeat(40)}})
    } else if line.contains("/pulls/42/reviews?") {
        serde_json::json!([{"id":1,"user":{"id":139_017,"type":"User","login":"owner"},"state":"APPROVED","commit_id":"c".repeat(40),"author_association":"OWNER","submitted_at":"2026-11-03T15:50:00Z"}])
    } else if line.contains("/collaborators/owner/permission") {
        serde_json::json!({"user":{"id":139_017,"type":"User"},"permission":"admin"})
    } else if line.starts_with("GET /repos/owner/project/actions/runs/12/approvals ") {
        serde_json::json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy} and source {source}, and authorize this source for run 12.")}])
    } else if line.starts_with("GET /repos/owner/project/actions/runs/12/attempts/1 ") {
        serde_json::json!({"id":12,"run_attempt":1,"status":"completed","head_sha":policy,"head_branch":"main","event":"workflow_dispatch","path":".github/workflows/crate-release.yml","repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}},"head_repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}}})
    } else if line.starts_with("GET /repos/owner/project/actions/runs/12 ") {
        serde_json::json!({"id":12,"run_attempt":1,"head_sha":policy,"head_branch":"main","event":"workflow_dispatch","path":".github/workflows/crate-release.yml","repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}},"head_repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}}})
    } else if line.contains("/compare/") {
        serde_json::json!({"status":"ahead"})
    } else if line
        .starts_with("GET /repos/owner/project/environments/release/deployment-branch-policies?")
    {
        serde_json::json!({"total_count":1,"branch_policies":[{"type":"branch","name":"main"}]})
    } else if line.starts_with("GET /repos/owner/project/environments/release ") {
        serde_json::json!({"id":9,"name":"release","can_admins_bypass":false,"deployment_branch_policy":{"custom_branch_policies":true},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":139_017}}]}]})
    } else if line.contains("/check-runs?") {
        let sha = if line.contains(policy) {
            policy
        } else {
            source
        };
        serde_json::json!({"total_count":1,"check_runs":[{"id":1,"name":"Required","head_sha":sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]})
    } else if line.contains("/actions/runs?") {
        let sha = if line.contains(policy) {
            policy
        } else {
            source
        };
        serde_json::json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success","event":"push","head_branch":"main","repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}},"head_repository":{"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"}}}]})
    } else {
        return None;
    };
    Some(value)
}

#[cfg(unix)]
#[derive(Default)]
pub(super) struct TestJournalState {
    pub(super) blobs: std::collections::BTreeMap<String, Vec<u8>>,
    tags: std::collections::BTreeMap<String, serde_json::Value>,
    refs: std::collections::BTreeMap<String, String>,
}

#[cfg(unix)]
pub(super) fn test_journal_response(
    line: &str,
    body: &[u8],
    state: &mut TestJournalState,
) -> Result<(&'static str, String), String> {
    use sha1_checked::{Digest as _, Sha1};
    let (status, response) = match line {
        path if path.starts_with("GET /repos/owner/project/git/matching-refs/tags/") => {
            ("200 OK", test_matching_refs(path, state)?)
        }
        "POST /repos/owner/project/git/tags HTTP/1.1" => {
            let value: serde_json::Value =
                serde_json::from_slice(body).map_err(|e| e.to_string())?;
            let blob = value["object"].as_str().ok_or("missing target")?;
            let tag_name = value["tag"].as_str().ok_or("missing tag")?;
            if !tag_name.starts_with("velnor-receipt/")
                || value["type"] != "blob"
                || !state.blobs.contains_key(blob)
            {
                return Err("invalid journal tag".into());
            }
            let raw = format!(
                "object {blob}\ntype blob\ntag {tag_name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
            );
            let mut hash = Sha1::new();
            hash.update(format!("tag {}\0", raw.len()));
            hash.update(raw.as_bytes());
            let digest = hash.try_finalize();
            if digest.has_collision() {
                return Err("journal hash collision".into());
            }
            let sha = digest
                .hash()
                .iter()
                .fold(String::new(), |mut output, byte| {
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                    output
                });
            let result = serde_json::json!({"sha":sha,"tag":tag_name,"message":"velnor-release-receipt-v1","object":{"type":"blob","sha":blob},"tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"},"verification":{"verified":false,"reason":"unsigned","signature":null,"payload":null}});
            state.tags.insert(sha, result.clone());
            ("201 Created", result)
        }
        path if path.starts_with("GET /repos/owner/project/git/tags/") => {
            let sha = path
                .strip_prefix("GET /repos/owner/project/git/tags/")
                .and_then(|value| value.strip_suffix(" HTTP/1.1"))
                .ok_or("invalid tag path")?;
            ("200 OK", state.tags.get(sha).ok_or("missing tag")?.clone())
        }
        "POST /repos/owner/project/git/refs HTTP/1.1" => {
            let value: serde_json::Value =
                serde_json::from_slice(body).map_err(|e| e.to_string())?;
            let sha = value["sha"].as_str().ok_or("missing tag sha")?;
            let reference = value["ref"].as_str().ok_or("missing ref")?;
            let tag = state.tags.get(sha).ok_or("missing tag")?;
            if tag["tag"]
                .as_str()
                .map(|name| format!("refs/tags/{name}"))
                .as_deref()
                != Some(reference)
            {
                return Err("invalid journal ref".into());
            }
            if state.refs.contains_key(reference) {
                return Ok(("422 Unprocessable Entity", "{}".into()));
            }
            state.refs.insert(reference.to_owned(), sha.into());
            (
                "201 Created",
                serde_json::json!({"ref":reference,"object":{"type":"tag","sha":sha}}),
            )
        }
        path if path.starts_with("GET /repos/owner/project/git/ref/tags/velnor-receipt%2F") => {
            return test_journal_ref(path, state);
        }
        _ => return Err(format!("unexpected request: {line}")),
    };
    Ok((status, response.to_string()))
}

#[cfg(unix)]
fn test_journal_ref(
    path: &str,
    state: &TestJournalState,
) -> Result<(&'static str, String), String> {
    let name = path
        .strip_prefix("GET /repos/owner/project/git/ref/tags/")
        .and_then(|v| v.strip_suffix(" HTTP/1.1"))
        .ok_or("invalid ref path")?
        .replace("%2F", "/");
    let reference = format!("refs/tags/{name}");
    let Some(sha) = state.refs.get(&reference) else {
        return Ok(("404 Not Found", "{}".into()));
    };
    Ok((
        "200 OK",
        serde_json::json!({"ref":reference,"object":{"type":"tag","sha":sha}}).to_string(),
    ))
}

#[cfg(unix)]
fn test_matching_refs(path: &str, state: &TestJournalState) -> Result<serde_json::Value, String> {
    let prefix = path
        .strip_prefix("GET /repos/owner/project/git/matching-refs/tags/")
        .and_then(|v| v.strip_suffix(" HTTP/1.1"))
        .ok_or("invalid prefix")?
        .replace("%2F", "/");
    let prefix = format!("refs/tags/{prefix}");
    let refs: Vec<_> = state
        .refs
        .iter()
        .filter(|(name, _)| name.starts_with(&prefix))
        .map(|(name, sha)| serde_json::json!({"ref":name,"object":{"type":"tag","sha":sha}}))
        .collect();
    Ok(serde_json::json!(refs))
}
