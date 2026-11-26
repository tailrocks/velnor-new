// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use url::Url;

use crate::context_value::ContextValue;
use crate::job_message::AgentJobRequestMessage;

#[derive(Debug, thiserror::Error)]
pub(crate) enum FactsError {
    #[error("trusted facts: {0}")]
    Json(#[from] serde_json::Error),
    #[error("trusted facts: {field} is missing")]
    Missing { field: &'static str },
    #[error("trusted facts: {field} '{value}' is not valid")]
    Invalid { field: &'static str, value: String },
}

fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('.')
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct RepoName(String);

impl RepoName {
    pub(crate) fn owner_and_name(&self) -> (&str, &str) {
        self.0.split_once('/').unwrap_or_default()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoName {
    type Error = FactsError;

    fn try_from(value: String) -> Result<Self, FactsError> {
        match value.split_once('/') {
            Some((owner, name)) if valid_part(owner) && valid_part(name) => Ok(Self(value)),
            _ => Err(FactsError::Invalid {
                field: "repository",
                value,
            }),
        }
    }
}

impl From<RepoName> for String {
    fn from(value: RepoName) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct CommitId(String);

impl CommitId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommitId {
    type Error = FactsError;

    fn try_from(value: String) -> Result<Self, FactsError> {
        let hex = value.bytes().all(|byte| byte.is_ascii_hexdigit());
        if hex && matches!(value.len(), 40 | 64) {
            Ok(Self(value.to_ascii_lowercase()))
        } else {
            Err(FactsError::Invalid {
                field: "sha",
                value,
            })
        }
    }
}

impl From<CommitId> for String {
    fn from(value: CommitId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct ServerUrl(Url);

impl ServerUrl {
    pub(crate) fn url(&self) -> &Url {
        &self.0
    }
}

impl TryFrom<String> for ServerUrl {
    type Error = FactsError;

    fn try_from(value: String) -> Result<Self, FactsError> {
        match Url::parse(&value) {
            Ok(url) if matches!(url.scheme(), "https" | "http" | "file") => Ok(Self(url)),
            _ => Err(FactsError::Invalid {
                field: "server_url",
                value,
            }),
        }
    }
}

impl From<ServerUrl> for String {
    fn from(value: ServerUrl) -> Self {
        value.0.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PullRequestHead {
    SameRepository,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TrustedEvent {
    Branch {
        branch: String,
    },
    Tag {
        name: String,
    },
    PullRequest {
        number: u64,
        base_branch: String,
        head: PullRequestHead,
    },
    MergeGroup {
        queue_branch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrustedFacts {
    pub(crate) repository_id: u64,
    pub(crate) repository: RepoName,
    pub(crate) server_url: ServerUrl,
    pub(crate) default_branch: String,
    pub(crate) sha: CommitId,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) event: TrustedEvent,
}

fn text<'a>(github: &'a ContextValue, path: &[&str]) -> Option<&'a str> {
    let mut value = github;
    for key in path {
        value = value.get(key)?;
    }
    match value {
        ContextValue::String(text) => Some(text),
        _ => None,
    }
}

fn number<T: std::str::FromStr + Default>(github: &ContextValue, key: &str) -> T {
    text(github, &[key])
        .and_then(|text| text.parse().ok())
        .unwrap_or_default()
}

fn id(github: &ContextValue, path: &[&str]) -> Option<u64> {
    let mut value = github;
    for key in path {
        value = value.get(key)?;
    }
    match value {
        ContextValue::String(text) => text.parse().ok(),
        ContextValue::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn pull_request_head(
    github: &ContextValue,
    repository: &RepoName,
) -> Result<PullRequestHead, FactsError> {
    let Some(name) = text(
        github,
        &["event", "pull_request", "head", "repo", "full_name"],
    ) else {
        return Ok(PullRequestHead::Fork);
    };
    let name = RepoName::try_from(name.to_owned())?;
    let head_id = id(github, &["event", "pull_request", "head", "repo", "id"]);
    let base_id = id(github, &["repository_id"]);
    let same = match (head_id, base_id) {
        (Some(head), Some(base)) => head == base,
        _ => name.as_str().eq_ignore_ascii_case(repository.as_str()),
    };
    Ok(if same {
        PullRequestHead::SameRepository
    } else {
        PullRequestHead::Fork
    })
}

fn required<'a>(github: &'a ContextValue, field: &'static str) -> Result<&'a str, FactsError> {
    text(github, &[field]).ok_or(FactsError::Missing { field })
}

fn event_of(github: &ContextValue, repository: &RepoName) -> Result<TrustedEvent, FactsError> {
    let git_ref = required(github, "ref")?;
    let pull = git_ref
        .strip_prefix("refs/pull/")
        .and_then(|rest| rest.strip_suffix("/merge"))
        .and_then(|number| number.parse().ok());
    if let Some(number) = pull {
        return Ok(TrustedEvent::PullRequest {
            number,
            base_branch: text(github, &["base_ref"]).unwrap_or_default().to_owned(),
            head: pull_request_head(github, repository)?,
        });
    }
    if let Some(name) = git_ref.strip_prefix("refs/tags/") {
        return Ok(TrustedEvent::Tag {
            name: name.to_owned(),
        });
    }
    match git_ref.strip_prefix("refs/heads/") {
        Some(branch) if branch.starts_with("gh-readonly-queue/") => Ok(TrustedEvent::MergeGroup {
            queue_branch: branch.to_owned(),
        }),
        Some(branch) => Ok(TrustedEvent::Branch {
            branch: branch.to_owned(),
        }),
        None => Err(FactsError::Invalid {
            field: "ref",
            value: git_ref.to_owned(),
        }),
    }
}

impl TrustedFacts {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, FactsError> {
        Ok(serde_json::from_slice(bytes)?)
    }

    pub(crate) fn from_job_message(message: &AgentJobRequestMessage) -> Result<Self, FactsError> {
        let contexts =
            message
                .materialize_context_values()
                .map_err(|error| FactsError::Invalid {
                    field: "context",
                    value: format!("{error:#}"),
                })?;
        let github = contexts.get("github").ok_or(FactsError::Missing {
            field: "github context",
        })?;
        let repository = RepoName::try_from(required(github, "repository")?.to_owned())?;
        let event = event_of(github, &repository)?;
        Ok(Self {
            repository_id: number(github, "repository_id"),
            server_url: ServerUrl::try_from(required(github, "server_url")?.to_owned())?,
            default_branch: text(github, &["event", "repository", "default_branch"])
                .unwrap_or_default()
                .to_owned(),
            sha: CommitId::try_from(required(github, "sha")?.to_owned())?,
            run_id: number(github, "run_id"),
            run_attempt: number(github, "run_attempt"),
            repository,
            event,
        })
    }

    pub(crate) fn clone_url(&self) -> String {
        format!(
            "{}/{}.git",
            self.server_url.url().as_str().trim_end_matches('/'),
            self.repository.as_str()
        )
    }

    pub(crate) fn refspecs(&self) -> Vec<String> {
        let mut specs = vec!["+refs/heads/*:refs/heads/*".to_owned()];
        if let TrustedEvent::PullRequest { number, .. } = &self.event {
            specs.push(format!("+{}:refs/pull/{number}/merge", self.sha.as_str()));
        }
        specs
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    use serde_json::{Value, json};

    use super::{PullRequestHead, TrustedEvent, TrustedFacts};
    use crate::job_message::AgentJobRequestMessage;

    const FIXTURE: &str = include_str!("../../fixtures/exec/checkout_pull_request.json");

    fn entry(key: &str, value: &Value) -> Value {
        json!({"k": key, "v": value})
    }

    fn string(value: &str) -> Value {
        json!({"t": 0, "s": value})
    }

    fn object(entries: &[Value]) -> Value {
        json!({"t": 2, "d": entries})
    }

    fn pull_request_head(
        base_id: Option<&str>,
        head_repo: Option<(&str, Option<u64>)>,
    ) -> Result<PullRequestHead> {
        let sha = "0123456789012345678901234567890123456789";
        let mut job: Value = serde_json::from_str(
            &FIXTURE
                .replace("@SHA@", sha)
                .replace("@SERVER_URL@", "https://github.com"),
        )?;
        let mut head = vec![entry("ref", &string("topic"))];
        if let Some((name, id)) = head_repo {
            let mut repo = vec![entry("full_name", &string(name))];
            if let Some(id) = id {
                repo.push(entry("id", &json!({"t": 4, "n": id})));
            }
            head.push(entry("repo", &object(&repo)));
        }
        let event = object(&[entry(
            "pull_request",
            &object(&[entry("head", &object(&head))]),
        )]);
        let github = job["ContextData"]["github"]["d"]
            .as_array_mut()
            .context("github context entries")?;
        github.push(entry("event", &event));
        if let Some(base_id) = base_id {
            github.push(entry("repository_id", &string(base_id)));
        }
        let message = AgentJobRequestMessage::parse_json(&job.to_string())?;
        match TrustedFacts::from_job_message(&message)?.event {
            TrustedEvent::PullRequest { head, .. } => Ok(head),
            other => anyhow::bail!("not a pull request event: {other:?}"),
        }
    }

    #[test]
    fn a_pull_request_head_is_the_base_repository_only_when_the_message_proves_it() -> Result<()> {
        use PullRequestHead::{Fork, SameRepository};
        let base = "tailrocks/velnor-new";
        let cases = [
            (None, None, Fork),
            (Some("1"), None, Fork),
            (None, Some((base, None)), SameRepository),
            (None, Some(("Tailrocks/Velnor-New", None)), SameRepository),
            (None, Some(("someone/velnor-new", None)), Fork),
            (Some("1"), Some((base, Some(1))), SameRepository),
            (Some("1"), Some((base, Some(2))), Fork),
            (
                Some("1"),
                Some(("someone/renamed", Some(1))),
                SameRepository,
            ),
            (Some("1"), Some((base, None)), SameRepository),
        ];
        for (base_id, head, expected) in cases {
            assert_eq!(
                pull_request_head(base_id, head)?,
                expected,
                "base id {base_id:?}, head {head:?}"
            );
        }
        Ok(())
    }
}
