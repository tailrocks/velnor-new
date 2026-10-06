//! Exact-source eligibility shared by every product release job.

use crate::yaml::Yaml;

use super::features::{CHECKOUT_USES, base, finish, run_step};

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const MISE_VERSION: &str = "2026.9.18";
const GH_VERSION: &str = "2.102.0";
const REPOSITORY: &str = "tailrocks/velnor-new";
const WORKFLOW_PATH: &str = ".github/workflows/product-release.yml";
const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";
const CI_WORKFLOW_API_ID: &str = "ci.yml";

const LATEST_RUN_JQ: &str = r#"
  [ .[] | (.workflow_runs // [])[] |
    select(.path == $workflow_path
      and .event == "push"
      and .head_branch == "main"
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
  [ .[].jobs[]? | select(.name == "Required") ] as $required
  | if ($required | length) != 1 then error("expected one Required job")
    else $required[0]
      | if .run_id == $run_id
          and .run_attempt == $run_attempt
          and .head_sha == $source_sha
          and .head_branch == "main"
        then .
        else error("Required job identity mismatch")
        end
    end
"#;

/// Job ID shared by product release coordinators.
pub const JOB_ID: &str = "release-eligibility";

/// Read-only source and required-CI gate. Its outputs bind all build jobs to
/// one immutable event SHA and identify the authority and CI attempt.
#[must_use]
pub fn job(runs_on: Yaml) -> (String, Yaml) {
    let mut fields = base("Check release source eligibility", runs_on, 70);
    fields.push((
        "permissions".to_owned(),
        Yaml::Map(vec![
            ("actions".to_owned(), Yaml::str("read")),
            ("contents".to_owned(), Yaml::str("read")),
        ]),
    ));
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "source_sha".to_owned(),
                Yaml::str("${{ steps.check.outputs.source_sha }}"),
            ),
            (
                "workflow_authority_sha".to_owned(),
                Yaml::str("${{ steps.check.outputs.workflow_authority_sha }}"),
            ),
            (
                "ci_run_id".to_owned(),
                Yaml::str("${{ steps.check.outputs.ci_run_id }}"),
            ),
            (
                "ci_attempt".to_owned(),
                Yaml::str("${{ steps.check.outputs.ci_attempt }}"),
            ),
        ]),
    ));
    finish(
        JOB_ID,
        fields,
        vec![
            checkout_step(),
            mise_step(),
            run_step(
                "Install pinned GitHub CLI",
                &format!("mise --no-config --no-env --no-hooks install gh@{GH_VERSION}"),
            ),
            check_step(),
        ],
    )
}

/// Bash gate run before build and again inside each publisher.
pub(super) fn script() -> String {
    ELIGIBILITY_SCRIPT
        .replace("@REPOSITORY@", REPOSITORY)
        .replace("@WORKFLOW_PATH@", WORKFLOW_PATH)
        .replace("@CI_WORKFLOW_PATH@", CI_WORKFLOW_PATH)
        .replace("@CI_WORKFLOW_API_ID@", CI_WORKFLOW_API_ID)
        .replace("@GH_VERSION@", GH_VERSION)
        .replace("@LATEST_RUN_JQ@", LATEST_RUN_JQ)
        .replace("@REQUIRED_JOB_JQ@", REQUIRED_JOB_JQ)
}

const ELIGIBILITY_SCRIPT: &str = r#"set -euo pipefail

readonly repository='@REPOSITORY@'
readonly workflow_path='@WORKFLOW_PATH@'
readonly ci_workflow_path='@CI_WORKFLOW_PATH@'
readonly source_sha="${GITHUB_SHA-}"
readonly authority_sha="${GITHUB_WORKFLOW_SHA-}"
readonly gh_version='@GH_VERSION@'

fail() {
  printf 'release eligibility: %s\n' "$1" >&2
  exit 1
}

is_sha() {
  local candidate="$1"
  [[ "${#candidate}" -eq 40 && "$candidate" != *[!0123456789abcdef]* ]]
}

[[ "${GITHUB_REPOSITORY-}" == "$repository" ]] || fail 'unexpected repository'
[[ "${GITHUB_REF-}" == 'refs/heads/main' ]] || fail 'source ref is not main'
[[ "${GITHUB_WORKFLOW_REF-}" == "$repository/$workflow_path@refs/heads/main" ]] || fail 'unexpected workflow authority path or ref'
case "${GITHUB_EVENT_NAME-}" in
  push|schedule|workflow_dispatch) ;;
  *) fail 'event is not eligible' ;;
esac
is_sha "$source_sha" || fail 'source SHA is malformed'
is_sha "$authority_sha" || fail 'workflow authority SHA is malformed'
[[ "$source_sha" == "$authority_sha" ]] || fail 'workflow authority and source differ'
[[ -n "${GH_TOKEN-}" ]] || fail 'read-only GitHub token is missing'
[[ -n "${GITHUB_OUTPUT-}" ]] || fail 'workflow output path is missing'

poll_limit="${VELNOR_RELEASE_CI_POLL_LIMIT:-240}"
poll_seconds="${VELNOR_RELEASE_CI_POLL_SECONDS:-15}"
[[ "$poll_limit" =~ ^[1-9][0-9]*$ ]] || fail 'poll limit is invalid'
[[ "$poll_seconds" =~ ^[0-9]+$ ]] || fail 'poll interval is invalid'

gh_api() {
  mise --no-config --no-env --no-hooks exec "gh@$gh_version" -- gh api "$@"
}

current_main_sha() {
  gh_api "repos/$repository/commits/main" | jq -er '.sha'
}

latest_ci_run() {
  gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/workflows/@CI_WORKFLOW_API_ID@/runs?head_sha=$source_sha&branch=main&event=push&per_page=100" \
    | jq -c \
        --arg source_sha "$source_sha" \
        --arg workflow_path "$ci_workflow_path" \
        --arg repository "$repository" \
        '@LATEST_RUN_JQ@'
}

assert_current_tip() {
  local current_sha
  current_sha="$(current_main_sha)"
  [[ "$current_sha" == "$source_sha" ]] || fail 'source is no longer the main tip'
}

assert_required_job() {
  local run_id="$1"
  local run_attempt="$2"
  local jobs required
  jobs="$(gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100")"
  required="$(printf '%s\n' "$jobs" | jq -c \
    --argjson run_id "$run_id" \
    --argjson run_attempt "$run_attempt" \
    --arg source_sha "$source_sha" \
    '@REQUIRED_JOB_JQ@')"
  [[ "$(jq -r '.status' <<<"$required")" == 'completed' ]] || fail 'Required job is not complete'
  [[ "$(jq -r '.conclusion' <<<"$required")" == 'success' ]] || fail 'Required job did not succeed'
  [[ "$(jq -r '.run_id' <<<"$required")" == "$run_id" ]] || fail 'Required job has a different run ID'
  [[ "$(jq -r '.head_sha' <<<"$required")" == "$source_sha" ]] || fail 'Required job has a different source SHA'
  [[ "$(jq -r '.head_branch' <<<"$required")" == 'main' ]] || fail 'Required job is not on main'
}

attempt=0
while [[ "$attempt" -lt "$poll_limit" ]]; do
  assert_current_tip
  run="$(latest_ci_run)"
  if [[ "$run" != 'null' ]]; then
    status="$(jq -r '.status' <<<"$run")"
    case "$status" in
      completed)
        [[ "$(jq -r '.conclusion' <<<"$run")" == 'success' ]] || fail 'latest exact-source CI run did not succeed'
        run_id="$(jq -r '.id' <<<"$run")"
        run_attempt="$(jq -r '.run_attempt' <<<"$run")"
        assert_required_job "$run_id" "$run_attempt"
        latest_again="$(latest_ci_run)"
        [[ "$(jq -r '.id' <<<"$latest_again")" == "$run_id" ]] || fail 'latest CI run changed during eligibility check'
        [[ "$(jq -r '.run_attempt' <<<"$latest_again")" == "$run_attempt" ]] || fail 'latest CI attempt changed during eligibility check'
        [[ "$(jq -r '.status' <<<"$latest_again")" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'
        [[ "$(jq -r '.conclusion' <<<"$latest_again")" == 'success' ]] || fail 'latest CI run changed during eligibility check'
        assert_current_tip
        printf 'source_sha=%s\n' "$source_sha" >> "$GITHUB_OUTPUT"
        printf 'workflow_authority_sha=%s\n' "$authority_sha" >> "$GITHUB_OUTPUT"
        printf 'ci_run_id=%s\n' "$run_id" >> "$GITHUB_OUTPUT"
        printf 'ci_attempt=%s\n' "$run_attempt" >> "$GITHUB_OUTPUT"
        exit 0
        ;;
      queued|in_progress|pending|waiting|requested) ;;
      *) fail 'latest exact-source CI run has an unknown status' ;;
    esac
  fi
  attempt=$((attempt + 1))
  if [[ "$attempt" -lt "$poll_limit" ]]; then
    sleep "$poll_seconds"
  fi
done
fail 'latest exact-source CI run did not become successful before timeout'
"#;

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out exact event source")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
            ]),
        ),
    ])
}

fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Set up pinned Mise")),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

fn check_step() -> Yaml {
    Yaml::Map(vec![
        ("id".to_owned(), Yaml::str("check")),
        (
            "name".to_owned(),
            Yaml::str("Verify main and latest Required CI"),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script())),
    ])
}

#[cfg(test)]
mod tests;
