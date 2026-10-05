//! Opaque cache-writer authority from event facts plus current GitHub evidence.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{CacheWriterFacts, Trust, WorkflowEvent};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::requests::PinnedToolExec;

const MAX_GH_OUTPUT_BYTES: usize = 256;
const MAX_EVENT_BYTES: usize = 1_048_576;
const GITHUB_API_HOSTNAME: &str = "github.com";

/// Verified cache-writer decision. Only this crate can create trusted values.
///
/// The type is intentionally not serializable. Callers can query it from
/// serializable [`CacheWriterFacts`], but cannot assert that GitHub reported a
/// protected branch themselves. [`Default`] is an untrusted cold context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheWriterContext {
    trust: Trust,
}

impl Default for CacheWriterContext {
    fn default() -> Self {
        Self { trust: Trust::Pr }
    }
}

impl CacheWriterContext {
    /// True only for this run's protected current-default-branch push.
    #[must_use]
    pub const fn permits_trusted_write(&self) -> bool {
        matches!(self.trust, Trust::Trusted)
    }

    /// True unless current event and GitHub API evidence authorize a write.
    #[must_use]
    pub const fn disproves_trusted_write(&self) -> bool {
        !self.permits_trusted_write()
    }

    /// Namespace selected after the verified API query.
    #[must_use]
    pub const fn trust(&self) -> Trust {
        self.trust
    }

    /// Join event facts with the current default branch and protection result.
    ///
    /// This constructor stays private: production contexts come only from
    /// [`context_for_facts`].
    fn from_observation(
        facts: &CacheWriterFacts,
        current_default_branch: Option<&str>,
        ref_protected: Option<bool>,
    ) -> Self {
        let trusted = current_default_branch
            .filter(|branch| valid_branch(branch).is_some())
            .is_some_and(|branch| {
                let protected_ref = format!("refs/heads/{branch}");
                facts.event == Some(WorkflowEvent::Push)
                    && ref_protected == Some(true)
                    && facts.default_branch.as_deref() == Some(branch)
                    && facts.git_ref.as_deref() == Some(protected_ref.as_str())
                    && facts
                        .repository
                        .as_deref()
                        .and_then(normalized_repo)
                        .zip(facts.event_repository.as_deref().and_then(normalized_repo))
                        .is_some_and(|(run_repo, event_repo)| run_repo == event_repo)
            });
        Self {
            trust: if trusted { Trust::Trusted } else { Trust::Pr },
        }
    }
}

/// Query the official GitHub API for current protected-default evidence.
///
/// Any mismatched input, missing credential, API failure, malformed response,
/// unknown protection state, or temporary-directory failure returns an
/// untrusted context. Callers continue normal work with cold cache behavior.
#[must_use]
pub fn context_for_facts(
    facts: &CacheWriterFacts,
    expected_event: WorkflowEvent,
    catalog: &ToolCatalog,
    cwd: &Path,
) -> CacheWriterContext {
    if expected_event != WorkflowEvent::Push
        || facts.event != Some(expected_event)
        || !runner_push_facts()
            .as_ref()
            .is_some_and(|runner_facts| request_matches_runner_facts(facts, runner_facts))
    {
        return CacheWriterContext::default();
    }
    let Some((repository, event_default, branch)) = api_query_inputs(facts) else {
        return CacheWriterContext::default();
    };
    let Some(current_default) = gh_query(catalog, cwd, repository_default_args(&repository)) else {
        return CacheWriterContext::default();
    };
    if current_default != event_default {
        return CacheWriterContext::from_observation(facts, Some(&current_default), None);
    }
    let Some(protected) = gh_query(catalog, cwd, branch_protection_args(&repository, &branch))
        .and_then(|value| parse_bool(&value))
    else {
        return CacheWriterContext::from_observation(facts, Some(&current_default), None);
    };
    CacheWriterContext::from_observation(facts, Some(&current_default), Some(protected))
}

/// Match request facts to the current runner-supplied event source exactly.
fn request_matches_runner_facts(request: &CacheWriterFacts, runner: &CacheWriterFacts) -> bool {
    request == runner
}

/// Re-read runner-owned push facts so serialized requests cannot mint authority.
fn runner_push_facts() -> Option<CacheWriterFacts> {
    if env::var("GITHUB_EVENT_NAME").ok()?.as_str() != "push" {
        return None;
    }
    let git_ref = env::var("GITHUB_REF").ok()?;
    let repository = env::var("GITHUB_REPOSITORY").ok()?;
    let event_path = env::var_os("GITHUB_EVENT_PATH").filter(|path| !path.is_empty())?;
    let payload_bytes = fs::read(event_path).ok()?;
    if payload_bytes.len() > MAX_EVENT_BYTES {
        return None;
    }
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    let payload_ref = payload["ref"].as_str()?;
    let default_branch = payload["repository"]["default_branch"].as_str()?;
    let event_repository = payload["repository"]["full_name"].as_str()?;
    if git_ref != payload_ref || !repository.eq_ignore_ascii_case(event_repository) {
        return None;
    }
    Some(CacheWriterFacts {
        event: Some(WorkflowEvent::Push),
        git_ref: Some(payload_ref.to_owned()),
        default_branch: Some(default_branch.to_owned()),
        repository: Some(repository),
        event_repository: Some(event_repository.to_owned()),
    })
}

/// Fixed current-repository default-branch API request.
fn repository_default_args(repository: &str) -> [OsString; 6] {
    [
        OsString::from("api"),
        OsString::from(format!("repos/{repository}")),
        OsString::from("--hostname"),
        OsString::from(GITHUB_API_HOSTNAME),
        OsString::from("--jq"),
        OsString::from(".default_branch"),
    ]
}

/// Fixed branch-protection API request with one encoded branch segment.
fn branch_protection_args(repository: &str, branch: &str) -> [OsString; 6] {
    [
        OsString::from("api"),
        OsString::from(format!("repos/{repository}/branches/{branch}")),
        OsString::from("--hostname"),
        OsString::from(GITHUB_API_HOSTNAME),
        OsString::from("--jq"),
        OsString::from(".protected"),
    ]
}

/// API target only when the run and event repositories agree on the default.
fn api_query_inputs(facts: &CacheWriterFacts) -> Option<(String, String, String)> {
    if facts.event != Some(WorkflowEvent::Push) {
        return None;
    }
    let repository = normalized_repo(facts.repository.as_deref()?)?;
    let event_repository = normalized_repo(facts.event_repository.as_deref()?)?;
    if repository != event_repository {
        return None;
    }
    let event_default = valid_branch(facts.default_branch.as_deref()?)?;
    let branch = facts
        .git_ref
        .as_deref()?
        .strip_prefix("refs/heads/")
        .and_then(valid_branch)?;
    if branch != event_default {
        return None;
    }
    Some((repository, event_default.to_owned(), encode_segment(branch)))
}

/// Run fixed pinned-gh requests with isolated per-call CLI configuration.
fn gh_query<const N: usize>(
    catalog: &ToolCatalog,
    cwd: &Path,
    args: [OsString; N],
) -> Option<String> {
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args.into()).ok()?;
    let config_dir = tempfile::tempdir().ok()?;
    let output = exec
        .command_with_isolated_gh_config(catalog, config_dir.path())
        .ok()?
        .with_cwd(PathBuf::from(cwd))
        .run()
        .ok()?;
    if !output.success || output.stdout.len() > MAX_GH_OUTPUT_BYTES {
        return None;
    }
    output
        .stdout_text("gh")
        .ok()
        .map(|text| text.trim().to_owned())
}

/// Strict boolean output from `gh api --jq .protected`.
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Normalized current repository slug.
fn normalized_repo(value: &str) -> Option<String> {
    let mut parts = value.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    (valid_part(owner) && valid_part(repo)).then(|| value.to_ascii_lowercase())
}

/// Strict branch name used in event/API comparison.
fn valid_branch(value: &str) -> Option<&str> {
    let bad = ["..", "//", "@{", "~", "^", ":", "?", "*", "[", "\\"];
    (!value.is_empty()
        && value != "HEAD"
        && !value.starts_with('/')
        && !value.starts_with('.')
        && !value.ends_with('/')
        && !value.ends_with('.')
        && !value.ends_with(".lock")
        && !value.chars().any(char::is_whitespace)
        && value
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
        && !bad.iter().any(|token| value.contains(token)))
    .then_some(value)
}

/// Percent-encode every non-unreserved byte so branch slashes remain one API segment.
fn encode_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
#[path = "cache_writer_tests.rs"]
mod tests;
