//! Trusted fetch operation context and strict canonical specification parsing.

use std::env;

use velnor_actions_contract::GitHubEvidenceFetchSpecV1;

use super::{Invocation, time};
use crate::OrchestratorError;
use crate::api::github_evidence_http::GitHubRepository;
use crate::internal::internal;

const SPEC_LIMIT: usize = 64 * 1024;

impl Invocation {
    pub(super) fn from_env() -> Result<Self, OrchestratorError> {
        Self::from_context(ContextValues::from_env())
    }

    pub(super) fn from_context(context: ContextValues) -> Result<Self, OrchestratorError> {
        if context.internal_op.as_deref()
            != Some(velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION)
        {
            return Err(internal("github_evidence_operation_identity"));
        }
        let spec_text = required(context.spec_text)?;
        if spec_text.len() > SPEC_LIMIT {
            return Err(internal("github_evidence_spec_size"));
        }
        let spec = GitHubEvidenceFetchSpecV1::parse_canonical_json(&spec_text)
            .map_err(|_| internal("github_evidence_spec_invalid"))?;
        let ref_name = required(context.ref_name)?;
        let sha = required(context.sha)?;
        let repository = GitHubRepository::parse(&required(context.repository)?)?;
        let repository_id = positive(context.repository_id)?;
        let run_id = positive(context.run_id)?;
        let run_attempt = positive(context.run_attempt)?;
        let token = required(context.token)?;
        if token.len() > 16 * 1024
            || context.event_name.as_deref() != Some("schedule")
            || ref_name != format!("refs/heads/{}", spec.default_branch)
            || !lower_hex(&sha, 40)
            || context.job.as_deref()
                != Some(format!("github-evidence-fetch-{}", spec.collector_suite_id).as_str())
            || !workflow_ref_matches(&spec, &repository, context.workflow_ref.as_deref())
        {
            return Err(internal("github_evidence_collector_context"));
        }
        let window = time::EvidenceWindow::new(spec.window_days)
            .map_err(|_| internal("github_evidence_clock"))?;
        Ok(Self {
            spec,
            token,
            repository,
            repository_id,
            run_id,
            run_attempt,
            ref_name,
            sha,
            window,
        })
    }
}

#[derive(Clone)]
pub(super) struct ContextValues {
    pub(super) internal_op: Option<String>,
    pub(super) spec_text: Option<String>,
    pub(super) ref_name: Option<String>,
    pub(super) sha: Option<String>,
    pub(super) repository: Option<String>,
    pub(super) repository_id: Option<String>,
    pub(super) run_id: Option<String>,
    pub(super) run_attempt: Option<String>,
    pub(super) token: Option<String>,
    pub(super) event_name: Option<String>,
    pub(super) job: Option<String>,
    pub(super) workflow_ref: Option<String>,
}

impl ContextValues {
    fn from_env() -> Self {
        Self {
            internal_op: env::var("VELNOR_INTERNAL_OP").ok(),
            spec_text: env::var("VELNOR_GITHUB_EVIDENCE_SPEC").ok(),
            ref_name: env::var("GITHUB_REF").ok(),
            sha: env::var("GITHUB_SHA").ok(),
            repository: env::var("GITHUB_REPOSITORY").ok(),
            repository_id: env::var("GITHUB_REPOSITORY_ID").ok(),
            run_id: env::var("GITHUB_RUN_ID").ok(),
            run_attempt: env::var("GITHUB_RUN_ATTEMPT").ok(),
            token: env::var("GH_TOKEN").ok(),
            event_name: env::var("GITHUB_EVENT_NAME").ok(),
            job: env::var("GITHUB_JOB").ok(),
            workflow_ref: env::var("GITHUB_WORKFLOW_REF").ok(),
        }
    }
}

fn workflow_ref_matches(
    spec: &GitHubEvidenceFetchSpecV1,
    repository: &GitHubRepository,
    value: Option<&str>,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some((repo_and_path, reference)) = value.rsplit_once('@') else {
        return false;
    };
    let Some((repo, path)) = repo_and_path.split_once("/.github/workflows/") else {
        return false;
    };
    repo.eq_ignore_ascii_case(&format!("{}/{}", repository.owner, repository.name))
        && path
            == spec
                .collector_workflow_path
                .trim_start_matches(".github/workflows/")
        && reference == format!("refs/heads/{}", spec.default_branch)
}

fn required(value: Option<String>) -> Result<String, OrchestratorError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("github_evidence_environment"))
}

fn positive<T>(value: Option<String>) -> Result<T, OrchestratorError>
where
    T: std::str::FromStr + PartialEq + Default,
{
    required(value)?
        .parse::<T>()
        .ok()
        .filter(|value| *value != T::default())
        .ok_or_else(|| internal("github_evidence_environment_number"))
}

fn lower_hex(value: &str, expected: usize) -> bool {
    value.len() == expected
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
