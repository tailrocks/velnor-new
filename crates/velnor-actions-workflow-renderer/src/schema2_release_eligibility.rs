// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::ci_run_lookup;
use super::features::{CHECKOUT_USES, base, finish};
use super::generator_release;
use super::{ProductReleasePins, ReleaseTarget};

pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const WORKFLOW_PATH: &str = ".github/workflows/product-release.yml";
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

pub const JOB_ID: &str = "release-eligibility";

pub fn job(runs_on: Yaml, pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Check release source eligibility",
        runs_on,
        JobTimeout::DEFAULT,
    );
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
    let mut steps = vec![
        checkout_step(),
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
    ];
    steps.extend(generator_release::release_gate_steps(pins)?);
    steps.push(check_step(pins)?);
    Ok(finish(JOB_ID, fields, steps))
}

pub(super) fn script(pins: &ProductReleasePins) -> Result<String, RenderError> {
    let gh_function = generator_release::gh_function(pins)?;
    Ok(format!(
        "set -euo pipefail\n\n{gh_function}\n\n{}\n\nrelease_eligibility\nexit 0\n",
        render_function(true)
    ))
}

pub(super) fn publisher_definition() -> String {
    render_function(true)
}

pub(super) fn publisher_definition_source_free() -> String {
    render_function(false)
}

fn render_function(require_checkout: bool) -> String {
    ELIGIBILITY_FUNCTION
        .replace("@REPOSITORY@", REPOSITORY)
        .replace("@WORKFLOW_PATH@", WORKFLOW_PATH)
        .replace("@CI_WORKFLOW_PATH@", CI_WORKFLOW_PATH)
        .replace("@CI_WORKFLOW_API_ID@", CI_WORKFLOW_API_ID)
        .replace("@LATEST_RUN_JQ@", LATEST_RUN_JQ)
        .replace("@REQUIRED_JOB_JQ@", REQUIRED_JOB_JQ)
        .replace("@CI_RUN_LISTING_FUNCTION@", &ci_run_lookup::bash_function())
        .replace(
            "@CHECKOUT_ASSERTION@",
            if require_checkout {
                "local checkout_sha\ncheckout_sha=\"$(git rev-parse HEAD)\" || fail 'cannot read checked-out source SHA'\n[[ \"$checkout_sha\" == \"$source_sha\" ]] || fail 'checked-out source SHA differs'"
            } else {
                ""
            },
        )
}

const ELIGIBILITY_FUNCTION: &str = r#"release_eligibility() {
local -r repository='@REPOSITORY@'
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
[[ "${GITHUB_REF-}" == 'refs/heads/main' ]] || fail 'source ref is not main'
[[ "${GITHUB_WORKFLOW_REF-}" == "$repository/@WORKFLOW_PATH@@refs/heads/main" ]] || fail 'unexpected workflow authority path or ref'
case "${GITHUB_EVENT_NAME-}" in
  workflow_dispatch) ;;
  *) fail 'event is not eligible' ;;
esac
is_sha "$source_sha" || fail 'source SHA is malformed'
is_sha "$authority_sha" || fail 'workflow authority SHA is malformed'
[[ "$source_sha" == "$authority_sha" ]] || fail 'workflow authority and source differ'
@CHECKOUT_ASSERTION@
[[ -n "${GH_TOKEN-}" ]] || fail 'read-only GitHub token is missing'
[[ -n "${GITHUB_OUTPUT-}" ]] || fail 'workflow output path is missing'

gh_api() {
  gh api "$@"
}

current_main_sha() {
  gh_api "repos/$repository/commits/main" | jq -er '.sha' \
    || fail 'main commit response is invalid'
}

@CI_RUN_LISTING_FUNCTION@

latest_ci_run() {
  local listing
  listing="$(list_exact_source_ci_runs 'release eligibility' gh_api --paginate --slurp -X GET \
    "repos/$repository/actions/workflows/@CI_WORKFLOW_API_ID@/runs?head_sha=$source_sha&per_page=100")" \
    || fail 'CI run lookup failed'
  printf '%s\n' "$listing" | jq -c \
      --arg source_sha "$source_sha" \
      --arg workflow_path "$ci_workflow_path" \
      --arg repository "$repository" \
      '@LATEST_RUN_JQ@' \
    || fail 'CI run response is invalid'
}

assert_current_tip() {
  local current_sha
  current_sha="$(current_main_sha)" || fail 'main commit lookup failed'
  [[ "$current_sha" == "$source_sha" ]] || fail 'source is no longer the main tip'
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
    --argjson run_attempt "$run_attempt" \
    --arg source_sha "$source_sha" \
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
  [[ "$actual_branch" == 'main' ]] || fail 'Required job is not on main'
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
assert_current_tip
{
  printf 'source_sha=%s\n' "$source_sha"
  printf 'workflow_authority_sha=%s\n' "$authority_sha"
  printf 'ci_run_id=%s\n' "$run_id"
  printf 'ci_attempt=%s\n' "$run_attempt"
} >> "$GITHUB_OUTPUT"
}"#;

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

fn check_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify main and latest Required CI"),
        ),
        ("id".to_owned(), Yaml::str("check")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script(pins)?)),
    ]))
}
