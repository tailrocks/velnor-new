// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::release_eligibility;

const WORKFLOW_PATH: &str = ".github/workflows/rust-ci.yml";
const WORKFLOW_API_ID: &str = "rust-ci.yml";
const REQUIRED_JOB_NAME: &str = "rust-required";

const GATE_JOB_JQ: &str = r#"
  [ .[].jobs[]? | select(.name == $job_name) ] as $matched
  | if ($matched | length) != 1 then error("expected one gate job")
    else $matched[0]
      | if .run_id == $run_id
          and .run_attempt == $run_attempt
          and .head_sha == $source_sha
          and .head_branch == "main"
        then .
        else error("gate job identity mismatch")
        end
    end
"#;

const FUNCTIONS: &str = r#"latest_gate_run() {
  local label="$1" workflow_path="$2" api_id="$3" listing
  listing="$(list_exact_source_ci_runs 'release eligibility' gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/workflows/$api_id/runs?head_sha=$source_sha&per_page=100")" \
    || fail "$label run lookup failed"
  printf '%s\n' "$listing" | jq -c \
      --arg source_sha "$source_sha" \
      --arg workflow_path "$workflow_path" \
      --arg repository "$repository" \
      '@LATEST_RUN_JQ@' \
    || fail "$label run response is invalid"
}

verify_gate() {
  local label="$1" workflow_path="$2" api_id="$3" job_name="$4"
  local gate_run gate_id gate_attempt jobs job again
  gate_run="$(latest_gate_run "$label" "$workflow_path" "$api_id")" \
    || fail "latest $label run lookup failed"
  [[ "$gate_run" != 'null' ]] || fail "latest exact-source $label run is missing"
  [[ "$(jq -er '.status' <<<"$gate_run")" == 'completed' ]] \
    || fail "latest exact-source $label run is not complete"
  [[ "$(jq -er '.conclusion' <<<"$gate_run")" == 'success' ]] \
    || fail "latest exact-source $label run did not succeed"
  gate_id="$(jq -er '.id' <<<"$gate_run")" || fail "latest $label run ID is invalid"
  gate_attempt="$(jq -er '.run_attempt' <<<"$gate_run")" \
    || fail "latest $label run attempt is invalid"
  jobs="$(gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/runs/$gate_id/attempts/$gate_attempt/jobs?per_page=100")" \
    || fail "$job_name job lookup failed"
  job="$(printf '%s\n' "$jobs" | jq -c \
    --argjson run_id "$gate_id" \
    --argjson run_attempt "$gate_attempt" \
    --arg source_sha "$source_sha" \
    --arg job_name "$job_name" \
    '@GATE_JOB_JQ@')" || fail "$job_name job response is invalid"
  [[ "$(jq -er '.status' <<<"$job")" == 'completed' ]] || fail "$job_name job is not complete"
  [[ "$(jq -er '.conclusion' <<<"$job")" == 'success' ]] || fail "$job_name job did not succeed"
  [[ "$(jq -er '.run_id' <<<"$job")" == "$gate_id" ]] || fail "$job_name job has a different run ID"
  [[ "$(jq -er '.head_sha' <<<"$job")" == "$source_sha" ]] \
    || fail "$job_name job has a different source SHA"
  again="$(latest_gate_run "$label" "$workflow_path" "$api_id")" \
    || fail "$label recheck failed"
  [[ "$(jq -er '.id' <<<"$again")" == "$gate_id" ]] \
    || fail "latest $label run changed during eligibility check"
  [[ "$(jq -er '.run_attempt' <<<"$again")" == "$gate_attempt" ]] \
    || fail "latest $label attempt changed during eligibility check"
  [[ "$(jq -er '.status' <<<"$again")" == 'completed' ]] \
    || fail "latest $label run restarted during eligibility check"
  [[ "$(jq -er '.conclusion' <<<"$again")" == 'success' ]] \
    || fail "latest $label run changed during eligibility check"
}"#;

pub(super) fn bash_functions() -> String {
    FUNCTIONS
        .replace("@GATE_JOB_JQ@", GATE_JOB_JQ)
        .replace("@LATEST_RUN_JQ@", release_eligibility::LATEST_RUN_JQ)
}

pub(super) fn call() -> String {
    format!("verify_gate 'Rust CI' '{WORKFLOW_PATH}' '{WORKFLOW_API_ID}' '{REQUIRED_JOB_NAME}'")
}
