#!/usr/bin/env bash
set -euo pipefail

readonly repository='tailrocks/velnor-new'
readonly workflow_path='.github/workflows/generator-release.yml'
readonly ci_workflow_path='.github/workflows/ci.yml'
readonly ci_workflow_id='ci.yml'
readonly gh_version='2.102.0'
readonly source_sha="${GITHUB_SHA-}"
readonly workflow_sha="${GITHUB_WORKFLOW_SHA-}"

fail() {
  printf 'generator release source: %s\n' "$1" >&2
  exit 1
}

is_sha() {
  [[ "${#1}" -eq 40 && "$1" != *[!0123456789abcdef]* ]]
}

gh_api() {
  mise --no-config --no-env --no-hooks exec "gh@$gh_version" -- gh api "$@"
}

current_default_branch_sha() {
  local encoded_branch
  encoded_branch="$(jq -nr --arg value "$default_branch" '$value | @uri')" \
    || fail 'could not encode default branch'
  gh_api "repos/$repository/branches/$encoded_branch" \
    | jq -er '.commit.sha | strings' \
    || fail 'could not read the default branch head'
}

latest_required_ci_run() {
  gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/workflows/$ci_workflow_id/runs" \
    -f "head_sha=$source_sha" \
    -f "branch=$default_branch" \
    -f 'event=push' \
    -F 'per_page=100' \
    | jq -cr \
        --arg source_sha "$source_sha" \
        --arg default_branch "$default_branch" \
        --arg workflow_path "$ci_workflow_path" \
        --arg repository "$repository" \
        '
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
        '
}

required_job() {
  local run_id="$1" run_attempt="$2" jobs
  jobs="$(gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs" \
    -F 'per_page=100')" || fail 'could not read Required CI jobs'
  jq -cer \
    --argjson run_id "$run_id" \
    --argjson run_attempt "$run_attempt" \
    --arg source_sha "$source_sha" \
    --arg default_branch "$default_branch" \
    '[ .[].jobs[]? | select(.name == "Required") ] as $matches
      | if ($matches | length) != 1 then error("expected one Required job")
        else $matches[0]
          | if .run_id == $run_id
              and .run_attempt == $run_attempt
              and .head_sha == $source_sha
              and .head_branch == $default_branch
            then .
            else error("Required job identity mismatch")
            end
        end' <<< "$jobs"
}

require_current_tip() {
  [[ "$(current_default_branch_sha)" == "$source_sha" ]] \
    || fail 'source is no longer the default branch head'
}

[[ "${GITHUB_REPOSITORY-}" == "$repository" ]] || fail 'unexpected repository'
[[ "${GITHUB_EVENT_NAME-}" == 'workflow_dispatch' ]] || fail 'release must be manually dispatched'
is_sha "$source_sha" || fail 'source SHA is malformed'
is_sha "$workflow_sha" || fail 'workflow SHA is malformed'
[[ "$workflow_sha" == "$source_sha" ]] || fail 'workflow and source SHAs differ'
[[ -n "${GH_TOKEN-}" ]] || fail 'read-only GitHub token is missing'
[[ -n "${GITHUB_OUTPUT-}" ]] || fail 'workflow output path is missing'

repository_json="$(gh_api "repos/$repository")" || fail 'could not read repository settings'
default_branch="$(jq -er '.default_branch | strings' <<< "$repository_json")" \
  || fail 'repository default branch is missing'
[[ -n "$default_branch" && "$default_branch" != *[$'\n'$'\r']* ]] \
  || fail 'repository default branch is malformed'
[[ "${GITHUB_REF-}" == "refs/heads/$default_branch" ]] \
  || fail 'event ref is not the current default branch'
[[ "${GITHUB_WORKFLOW_REF-}" == "$repository/$workflow_path@refs/heads/$default_branch" ]] \
  || fail 'workflow is not running from the default branch'

require_current_tip
poll_limit="${VELNOR_RELEASE_CI_POLL_LIMIT:-240}"
poll_seconds="${VELNOR_RELEASE_CI_POLL_SECONDS:-15}"
[[ "$poll_limit" =~ ^[1-9][0-9]*$ ]] || fail 'CI poll limit is invalid'
[[ "$poll_seconds" =~ ^[0-9]+$ ]] || fail 'CI poll interval is invalid'

attempt=0
while [[ "$attempt" -lt "$poll_limit" ]]; do
  run="$(latest_required_ci_run)" || fail 'could not read exact-source CI runs'
  if [[ "$run" != 'null' ]]; then
    status="$(jq -er '.status | strings' <<< "$run")" || fail 'CI run status is malformed'
    case "$status" in
      completed)
        [[ "$(jq -er '.conclusion | strings' <<< "$run")" == 'success' ]] \
          || fail 'latest exact-source CI run did not succeed'
        run_id="$(jq -er '.id | numbers' <<< "$run")" || fail 'CI run ID is malformed'
        run_attempt="$(jq -er '.run_attempt | numbers' <<< "$run")" \
          || fail 'CI run attempt is malformed'
        job="$(required_job "$run_id" "$run_attempt")" \
          || fail 'Required job identity check failed'
        [[ "$(jq -er '.status | strings' <<< "$job")" == 'completed' ]] \
          || fail 'Required job is not complete'
        [[ "$(jq -er '.conclusion | strings' <<< "$job")" == 'success' ]] \
          || fail 'Required job did not succeed'
        latest_again="$(latest_required_ci_run)" \
          || fail 'could not recheck exact-source CI run'
        [[ "$(jq -er '.id | numbers' <<< "$latest_again")" == "$run_id" ]] \
          || fail 'latest CI run changed during source check'
        [[ "$(jq -er '.run_attempt | numbers' <<< "$latest_again")" == "$run_attempt" ]] \
          || fail 'latest CI attempt changed during source check'
        [[ "$(jq -er '.status | strings' <<< "$latest_again")" == 'completed' \
          && "$(jq -er '.conclusion | strings' <<< "$latest_again")" == 'success' ]] \
          || fail 'latest CI run changed during source check'
        require_current_tip
        printf 'source_sha=%s\n' "$source_sha" >> "$GITHUB_OUTPUT"
        printf 'default_branch=%s\n' "$default_branch" >> "$GITHUB_OUTPUT"
        printf 'workflow_sha=%s\n' "$workflow_sha" >> "$GITHUB_OUTPUT"
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
