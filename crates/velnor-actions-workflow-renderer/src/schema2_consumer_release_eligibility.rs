//! Generic exact-source eligibility gate for consumer binary releases.

use crate::RenderError;
use crate::commands::join_argv_for_run;

use super::ci_run_lookup;
use velnor_actions_contract::package_release::PackageReleaseChannel;

use velnor_actions_contract::CI_WORKFLOW_PATH;
#[path = "schema2_consumer_preview_trigger.rs"]
pub(super) mod preview_trigger;
const CI_WORKFLOW_API_ID: &str = "ci.yml";

const LATEST_RUN_JQ: &str = r#"
  [ .[] | (.workflow_runs // [])[] |
    select(.path == $workflow_path
      and .event == "push"
      and .head_branch == $default_branch
      and .head_sha == $source_sha
      and .head_repository.full_name == $repository
      and (.id | type) == "number"
      and (.run_number | type) == "number"
      and (.run_attempt | type) == "number")
  ] as $runs
  | if ($runs | length) == 0 then null
    else
      ($runs | sort_by([.run_number, .id])) as $ordered
      | $ordered[-1] as $latest
      | [$ordered[] | select(.run_number == $latest.run_number)] as $same_number
      | if ($same_number | length) != 1
        then error("ambiguous CI run number")
        else $latest
        end
    end
"#;

const REQUIRED_JOB_JQ: &str = r#"
  [ .[].jobs[]? | select(.name == $required_job) ] as $required
  | if ($required | length) != 1 then error("expected one Required job")
    else $required[0]
      | if .run_id == $run_id
          and .head_sha == $source_sha
          and .head_branch == $default_branch
        then .
        else error("Required job identity mismatch")
        end
    end
"#;

/// Consumer identity passed to the exact-source eligibility script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseEligibilityContext {
    /// Repository derived from local `origin`.
    pub repository: String,
    /// Expected live default branch, resolved from `origin/HEAD`.
    pub default_branch: String,
    /// Native channel owns its exact generated workflow path and trigger policy.
    pub channel: PackageReleaseChannel,
}

/// Render the shared source and Required-CI gate for one consumer repository.
///
/// # Errors
///
/// Returns an error for invalid identity values or CLI arguments.
pub(super) fn consumer_script(
    context: &ReleaseEligibilityContext,
    gh_argv: &[String],
    publisher: bool,
) -> Result<String, RenderError> {
    if !valid_repository(&context.repository)
        || !velnor_actions_contract::is_valid_branch_name(&context.default_branch)
    {
        return Err(RenderError::InvalidWorkflow(
            "binary_release_eligibility_context_invalid".to_owned(),
        ));
    }
    let executable = join_argv_for_run(gh_argv)?;
    let gh_function = format!(
        "gh() {{\n  (\n    {executable} \"$@\" & _velnor_gh_pid=$!\n    ( sleep 60; kill -TERM \"$_velnor_gh_pid\" 2>/dev/null; sleep 5; kill -KILL \"$_velnor_gh_pid\" 2>/dev/null ) </dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!\n    _velnor_gh_status=0\n    wait \"$_velnor_gh_pid\" || _velnor_gh_status=$?\n    kill -KILL \"$_velnor_gh_watch\" 2>/dev/null || true\n    wait \"$_velnor_gh_watch\" 2>/dev/null || true\n    exit \"$_velnor_gh_status\"\n  ) </dev/null\n}}\nexport -f gh"
    );
    Ok(ELIGIBILITY_SCRIPT
        .replace("@GH_FUNCTION@", &gh_function)
        .replace("@CI_RUN_LISTING_FUNCTION@", &ci_run_lookup::bash_function())
        .replace("@REPOSITORY@", &context.repository)
        .replace("@DEFAULT_BRANCH@", &context.default_branch)
        .replace("@WORKFLOW_PATH@", context.channel.workflow_path())
        .replace(
            "@ELIGIBLE_EVENTS@",
            match context.channel {
                PackageReleaseChannel::Stable => "workflow_dispatch",
                PackageReleaseChannel::Preview => "workflow_run|workflow_dispatch",
            },
        )
        .replace(
            "@PREVIEW_FUNCTION@",
            preview_trigger::admission(context.channel),
        )
        .replace(
            "@PREVIEW_ASSERT@",
            preview_trigger::assertion(context.channel),
        )
        .replace(
            "@CI_WORKFLOW_NAME@",
            velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
        )
        .replace("@CI_WORKFLOW_PATH@", CI_WORKFLOW_PATH)
        .replace("@CI_WORKFLOW_API_ID@", CI_WORKFLOW_API_ID)
        .replace("@LATEST_RUN_JQ@", LATEST_RUN_JQ)
        .replace("@REQUIRED_JOB_JQ@", REQUIRED_JOB_JQ)
        .replace(
            "@CALL@",
            if publisher {
                "release_eligibility"
            } else {
                "release_eligibility\nexit 0"
            },
        ))
}

fn valid_repository(repository: &str) -> bool {
    let Some((owner, name)) = repository.split_once('/') else {
        return false;
    };
    !name.contains('/')
        && [owner, name].into_iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}

const ELIGIBILITY_SCRIPT: &str = r#"set -euo pipefail

@GH_FUNCTION@

release_eligibility() {
local -r repository='@REPOSITORY@'
local -r default_branch='@DEFAULT_BRANCH@'
local -r ci_workflow_path='@CI_WORKFLOW_PATH@'
local -r source_sha="${GITHUB_SHA-}"
local -r authority_sha="${GITHUB_WORKFLOW_SHA-}"

fail() {
  printf 'release eligibility: %s\n' "$1" >&2
  exit 1
}

is_sha() {
  local candidate="$1"
  [[ "${#candidate}" -eq 40 && "$candidate" != *[!0123456789abcdef]* ]]
}

[[ "${GITHUB_REPOSITORY-}" == "$repository" ]] || fail 'unexpected repository'
[[ "${GITHUB_REF-}" == "refs/heads/$default_branch" ]] || fail 'source ref is not the default branch'
[[ "${GITHUB_WORKFLOW_REF-}" == "$repository/@WORKFLOW_PATH@@refs/heads/$default_branch" ]] || fail 'unexpected workflow authority path or ref'
case "${GITHUB_EVENT_NAME-}" in
  @ELIGIBLE_EVENTS@) ;;
  *) fail 'event is not eligible' ;;
esac
is_sha "$source_sha" || fail 'source SHA is malformed'
is_sha "$authority_sha" || fail 'workflow authority SHA is malformed'
[[ "$source_sha" == "$authority_sha" ]] || fail 'workflow authority and source differ'
local checkout_sha
checkout_sha="$(git rev-parse HEAD)" || fail 'cannot read checked-out source SHA'
[[ "$checkout_sha" == "$source_sha" ]] || fail 'checked-out source SHA differs'
[[ -n "${GH_TOKEN-}" ]] || fail 'read-only GitHub token is missing'
[[ -n "${GITHUB_OUTPUT-}" ]] || fail 'workflow output path is missing'

gh_api() {
  gh api "$@"
}

@PREVIEW_FUNCTION@

assert_default_branch() {
  local current_branch
  current_branch="$(gh_api "repos/$repository" | jq -er '.default_branch')" \
    || fail 'default branch response is invalid'
  [[ "$current_branch" == "$default_branch" ]] || fail 'repository default branch changed'
}

current_default_branch_sha() {
  gh_api -X GET "repos/$repository/commits" -f "sha=$default_branch" -F per_page=1 \
    | jq -er 'if length == 1 then .[0].sha else error("expected one default-branch commit") end' \
    || fail 'default branch commit response is invalid'
}

@CI_RUN_LISTING_FUNCTION@

latest_ci_run() {
  local listing
  listing="$(list_exact_source_ci_runs 'release eligibility' gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/workflows/@CI_WORKFLOW_API_ID@/runs" \
    -f "head_sha=$source_sha" -F per_page=100)" \
    || fail 'CI run lookup failed'
  printf '%s\n' "$listing" | jq -c \
      --arg source_sha "$source_sha" \
      --arg workflow_path "$ci_workflow_path" \
      --arg repository "$repository" \
      --arg default_branch "$default_branch" \
      '@LATEST_RUN_JQ@' \
    || fail 'CI run response is invalid'
}

assert_current_tip() {
  assert_default_branch
  local current_sha
  current_sha="$(current_default_branch_sha)" || fail 'default branch commit lookup failed'
  [[ "$current_sha" == "$source_sha" ]] || fail 'source is no longer the default branch tip'
}

assert_required_job() {
  local run_id="$1"
  local run_attempt="$2"
  local jobs required
  jobs="$(gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100")" \
    || fail 'Required job lookup failed'
  required="$(printf '%s\n' "$jobs" | jq -c \
    --argjson run_id "$run_id" \
    --arg source_sha "$source_sha" \
    --arg default_branch "$default_branch" \
    --arg required_job "Required" \
    '@REQUIRED_JOB_JQ@')" || fail 'Required job response is invalid'
  local status conclusion actual_run actual_sha actual_branch
  status="$(jq -er '.status' <<<"$required")" || fail 'Required job status is invalid'
  conclusion="$(jq -er '.conclusion' <<<"$required")" \
    || fail 'Required job conclusion is invalid'
  actual_run="$(jq -er '.run_id' <<<"$required")" || fail 'Required job run ID is invalid'
  actual_sha="$(jq -er '.head_sha' <<<"$required")" || fail 'Required job source SHA is invalid'
  actual_branch="$(jq -er '.head_branch' <<<"$required")" \
    || fail 'Required job branch is invalid'
  [[ "$status" == 'completed' ]] || fail 'Required job is not complete'
  [[ "$conclusion" == 'success' ]] || fail 'Required job did not succeed'
  [[ "$actual_run" == "$run_id" ]] || fail 'Required job has a different run ID'
  [[ "$actual_sha" == "$source_sha" ]] || fail 'Required job has a different source SHA'
  [[ "$actual_branch" == "$default_branch" ]] || fail 'Required job is not on the default branch'
}

assert_current_tip
run="$(latest_ci_run)" || fail 'latest CI run lookup failed'
[[ "$run" != 'null' ]] || fail 'latest exact-source CI run is missing'
status="$(jq -er '.status' <<<"$run")" || fail 'latest CI run status is invalid'
[[ "$status" == 'completed' ]] || fail 'latest exact-source CI run is not complete'
run_conclusion="$(jq -er '.conclusion' <<<"$run")" \
  || fail 'latest CI run conclusion is invalid'
[[ "$run_conclusion" == 'success' ]] || fail 'latest exact-source CI run did not succeed'
run_id="$(jq -er '.id' <<<"$run")" || fail 'latest CI run ID is invalid'
run_attempt="$(jq -er '.run_attempt' <<<"$run")" \
  || fail 'latest CI run attempt is invalid'
@PREVIEW_ASSERT@
assert_required_job "$run_id" "$run_attempt"
latest_again="$(latest_ci_run)" || fail 'latest CI recheck failed'
latest_id="$(jq -er '.id' <<<"$latest_again")" || fail 'latest CI recheck ID is invalid'
latest_attempt="$(jq -er '.run_attempt' <<<"$latest_again")" \
  || fail 'latest CI recheck attempt is invalid'
latest_status="$(jq -er '.status' <<<"$latest_again")" \
  || fail 'latest CI recheck status is invalid'
latest_conclusion="$(jq -er '.conclusion' <<<"$latest_again")" \
  || fail 'latest CI recheck conclusion is invalid'
[[ "$latest_id" == "$run_id" ]] || fail 'latest CI run changed during eligibility check'
[[ "$latest_attempt" == "$run_attempt" ]] || fail 'latest CI attempt changed during eligibility check'
[[ "$latest_status" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'
[[ "$latest_conclusion" == 'success' ]] || fail 'latest CI run changed during eligibility check'
@PREVIEW_ASSERT@
assert_current_tip
{
  printf 'source_sha=%s\n' "$source_sha"
  printf 'workflow_authority_sha=%s\n' "$authority_sha"
  printf 'ci_run_id=%s\n' "$run_id"
  printf 'ci_attempt=%s\n' "$run_attempt"
} >> "$GITHUB_OUTPUT"
}

@CALL@
"#;

#[cfg(test)]
#[path = "schema2_consumer_release_eligibility_tests.rs"]
mod tests;
