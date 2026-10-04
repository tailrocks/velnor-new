//! Live child-run binding, cancellation, and terminal-state polling.

use super::CACHE_SNAPSHOT_FUNCTION;

const API_COMMON: &str = r#"set -euo pipefail
umask 077
root="$RUNNER_TEMP/mbx-cancel-controller"
private_root_open "$root"
trap 'private_remove_file "$root" "$root/artifact-response.headers" 65536 || true
  private_remove_file "$root" "$root/signed-response.headers" 65536 || true' EXIT
gh_api() { gh api --hostname github.com "$@"; }
expected_title="MBX cancellation $VICTIM_MODE $PROBE_ID"
valid_id() { [[ "$1" =~ ^[1-9][0-9]*$ ]]; }
run_valid() {
  # Split the workflow-run REST values documented at apiVersion=2022-11-28.
  # https://docs.github.com/en/rest/actions/workflow-runs?apiVersion=2022-11-28
  jq -e --argjson id "$RUN_ID" --argjson workflow "$WORKFLOW_ID" \
    --arg repo "$GITHUB_REPOSITORY" --arg sha "$GITHUB_SHA" \
    --arg title "$expected_title" \
    '.id == $id and .workflow_id == $workflow and .repository.full_name == $repo
     and .head_repository.full_name == $repo
     and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
     and .run_attempt == 1 and .display_title == $title
     and (.status | type == "string"
       and IN("queued", "in_progress", "requested", "waiting", "pending", "completed"))
     and (if .status == "completed" then
       (.conclusion | type == "string"
         and IN("action_required", "cancelled", "failure", "neutral", "skipped", "stale", "success", "timed_out"))
       else has("conclusion") and .conclusion == null end)
     and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
     and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
    "$1" >/dev/null 2>&1
}
fetch_run() { private_gh_json "$root" "$1" --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID"; }
fetch_jobs() {
  private_gh_json "$root" "$1" --method GET \
    "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/attempts/1/jobs?per_page=100" \
    && private_list_complete "$1" jobs
}
job_count() { jq -er --arg name "$VICTIM_JOB_NAME" '[.jobs[] | select(.name == $name)] | length' "$1"; }
job_id() { jq -er --arg name "$VICTIM_JOB_NAME" '[.jobs[] | select(.name == $name)] | if length == 1 then .[0].id else error("job identity") end' "$1"; }
job_state() { jq -er --arg name "$VICTIM_JOB_NAME" '[.jobs[] | select(.name == $name)] | if length == 1 then .[0].status else error("job identity") end' "$1"; }
target_live() {
  jq -e --arg name "$VICTIM_JOB_NAME" --arg step "$CANCEL_STEP_NAME" \
    '[.jobs[] | select(.name == $name)] as $jobs | ($jobs | length) == 1
     and $jobs[0].status == "in_progress"
     and ([ $jobs[0].steps[] | select(.name == $step and .status == "in_progress") ] | length) == 1' \
    "$1" >/dev/null 2>&1
}
target_finished() {
  jq -e --arg name "$VICTIM_JOB_NAME" --arg step "$CANCEL_STEP_NAME" \
    '[.jobs[] | select(.name == $name)] as $jobs | ($jobs | length) == 1
     and ([ $jobs[0].steps[] | select(.name == $step and .status == "completed") ] | length) == 1' \
    "$1" >/dev/null 2>&1
}
"#;

const WAIT_READINESS_BODY: &str = r#"
ready=false
reason=identity_or_window_missing
if ! valid_id "${RUN_ID:-}" || ! valid_id "${WORKFLOW_ID:-}"; then
  printf 'ready=false\nreason=dispatch_identity_missing\n' >> "$GITHUB_OUTPUT"
  exit 0
fi
for try in $(seq 1 180); do
  if ! fetch_run "$root/run.json"; then
    reason=exact_run_identity_unavailable
    sleep 5
    continue
  fi
  if ! run_valid "$root/run.json"; then
    reason=exact_run_identity_mismatch
    break
  fi
  if ! fetch_jobs "$root/jobs.json"; then sleep 5; continue; fi
  if [ "$(job_count "$root/jobs.json")" != 1 ]; then sleep 5; continue; fi
  if [ "$(job_state "$root/jobs.json")" != in_progress ]; then
    reason=victim_not_in_progress
    if [ "$(job_state "$root/jobs.json")" = completed ]; then break; fi
    sleep 5; continue
  fi
  if ! target_live "$root/jobs.json"; then
    if target_finished "$root/jobs.json"; then reason=cancel_window_already_finished; break; fi
    sleep 5; continue
  fi
  if ! receipt_step_complete "$root/jobs.json"; then sleep 5; continue; fi
  if validate_victim_artifact; then
    ready=true
    reason=exact_identity_and_cancel_window_ready
    break
  fi
  case "$ARTIFACT_CHECK" in invalid) reason=victim_receipt_invalid; break ;; esac
  sleep 5
done
printf 'ready=%s\nreason=%s\n' "$ready" "$reason" >> "$GITHUB_OUTPUT"
"#;

const RECEIPT_HELPERS: &str = r#"
receipt_step_complete() {
  jq -e --arg job "$VICTIM_JOB_NAME" \
    '[.jobs[] | select(.name == $job)] as $jobs | ($jobs | length) == 1
     and ([ $jobs[0].steps[] | select(.name == "Write MBX cancellation readiness receipt" and .conclusion == "success") ] | length) == 1
     and ([ $jobs[0].steps[] | select(.name == "Upload MBX cancellation receipt" and .conclusion == "success") ] | length) == 1' \
    "$1" >/dev/null 2>&1
}
validate_key() {
  local key rustc_identity scope_hash expected
  key="$(jq -er '.cache_key | strings' "$root/readiness.json")" || return 1
  rustc_identity="$(jq -er '.rustc_identity' "$root/readiness.json")" || return 1
  case "$rustc_identity" in ''|*[!0-9a-f]*) return 1 ;; esac
  [[ "$rustc_identity" =~ ^[0-9a-f]{64}$ ]] || return 1
  test "$MBX_GENERATION" = "velnor-mbx-$MBX_VERSION" || return 1
  scope_hash="$(printf '%s\n%s\n%s\n' \
    "$GITHUB_REPOSITORY/.github/workflows/qualification.yml" "$CACHE_SCOPE" '{}' \
    | sha256sum | cut -d' ' -f1)" || return 1
  expected="linux-x64-mbx-$MBX_GENERATION-dir-rust-$RUST_VERSION-$rustc_identity-scope-$scope_hash-run-$RUN_ID-attempt-1-$GITHUB_SHA"
  test "$key" = "$expected"
}
artifact_location() {
  local artifact_id="$1" response="$root/artifact-response.headers" status_line location_count location
  test -n "${GH_TOKEN:-}" || return 1
  private_capture "$root" "$response" 65536 curl --disable --noproxy '*' \
    --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
    --max-filesize 65536 --silent --show-error --fail --dump-header - --output /dev/null \
    --write-out '\nSTATUS:%{http_code}\n' \
    --header "Authorization: Bearer $GH_TOKEN" \
    --header 'Accept: application/vnd.github+json' \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    "https://api.github.com/repos/tailrocks/velnor-new/actions/artifacts/$artifact_id/zip" \
    || return 1
  private_file_valid "$root" "$response" 65536 || return 1
  status_line="$(awk '/^STATUS:/ { sub(/^STATUS:/, ""); print }' "$response")"
  test "$status_line" = 302 || return 1
  location_count="$(awk 'tolower($1) == "location:" { count++ } END { print count+0 }' "$response")"
  test "$location_count" = 1 || return 1
  location="$(awk 'tolower($1) == "location:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print }' "$response")"
  stock_restore_safe_https_url "$location" || return 1
  ARTIFACT_LOCATION="$location"
}
bounded_header_curl() { (ulimit -f 128; curl "$@"); }
download_signed_artifact() {
  local destination="$1" headers="$root/signed-response.headers" status
  private_capture "$root" "$headers" 65536 printf '' || return 1
  private_capture "$root" "$destination" 1048576 bounded_header_curl --disable --noproxy '*' \
    --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 45 \
    --max-filesize 1048576 --silent --show-error --dump-header "$headers" \
    --output - -- "$ARTIFACT_LOCATION" 2>/dev/null || return 1
  private_file_valid "$root" "$headers" 65536 || return 1
  status="$(awk '$1 ~ /^HTTP\// { status=$2 } END { print status }' "$headers")"
  test "$status" = 200
}
validate_victim_artifact() {
  local run_path="${1:-$root/run.json}"
  local artifact_id artifact_size download_size digest actual members member_count statuses content_size
  ARTIFACT_CHECK=missing
  if ! private_gh_json "$root" "$root/artifacts.json" --method GET \
      "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/artifacts?per_page=100" 2>/dev/null \
      || ! private_list_complete "$root/artifacts.json" artifacts; then return 1; fi
  if ! jq -e '
    all(.artifacts[]; type == "object"
      and (.id | type == "number" and . > 0 and . == floor)
      and (.name | type == "string" and length > 0 and length <= 128)
      and (.expired | type == "boolean")
      and (.size_in_bytes | type == "number" and . > 0 and . <= 1048576 and . == floor)
      and (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$"))
      and (.workflow_run | type == "object" and (.id | type == "number" and . > 0 and . == floor)))' \
      "$root/artifacts.json" >/dev/null 2>&1; then ARTIFACT_CHECK=invalid; return 1; fi
  member_count="$(jq -er --arg name "$VICTIM_ARTIFACT_NAME" --argjson id "$RUN_ID" \
    '[.artifacts[] | select(.name == $name and .expired == false and .workflow_run.id == $id)] | length' \
    "$root/artifacts.json" 2>/dev/null || printf 0)"
  case "$member_count" in
    0) return 1 ;;
    1) ;;
    *) ARTIFACT_CHECK=invalid; return 1 ;;
  esac
  artifact_id="$(jq -er --arg name "$VICTIM_ARTIFACT_NAME" --argjson id "$RUN_ID" \
    '[.artifacts[] | select(.name == $name and .expired == false and .workflow_run.id == $id)] | .[0].id | select(type == "number" and . > 0)' \
    "$root/artifacts.json")" || { ARTIFACT_CHECK=invalid; return 1; }
  artifact_size="$(jq -er --arg name "$VICTIM_ARTIFACT_NAME" --argjson id "$RUN_ID" \
    '[.artifacts[] | select(.name == $name and .expired == false and .workflow_run.id == $id)] | .[0].size_in_bytes | select(type == "number" and . > 0 and . <= 1048576 and . == floor)' \
    "$root/artifacts.json")" || { ARTIFACT_CHECK=invalid; return 1; }
  digest="$(jq -er --arg name "$VICTIM_ARTIFACT_NAME" --argjson id "$RUN_ID" \
    '[.artifacts[] | select(.name == $name and .expired == false and .workflow_run.id == $id)] | .[0].digest | strings | sub("^sha256:"; "") | select(test("^[0-9a-f]{64}$"))' \
    "$root/artifacts.json")" || { ARTIFACT_CHECK=invalid; return 1; }
  if [ -e "$root/validated-artifact.json" ] || [ -L "$root/validated-artifact.json" ]; then
    private_json_valid "$root" "$root/validated-artifact.json" 1024 \
      || { ARTIFACT_CHECK=invalid; return 1; }
    jq -e --arg id "$artifact_id" --arg digest "$digest" --argjson size "$artifact_size" \
      '.id == $id and .digest == $digest and .size_in_bytes == $size' \
      "$root/validated-artifact.json" >/dev/null 2>&1 || { ARTIFACT_CHECK=invalid; return 1; }
  fi
  if ! artifact_location "$artifact_id" || ! download_signed_artifact "$root/victim.zip"; then
    ARTIFACT_CHECK=invalid
    return 1
  fi
  private_remove_file "$root" "$root/artifact-response.headers" 65536 || return 1
  private_remove_file "$root" "$root/signed-response.headers" 65536 || return 1
  private_file_valid "$root" "$root/victim.zip" 1048576 \
    || { ARTIFACT_CHECK=invalid; return 1; }
  download_size="$(private_stat size "$root/victim.zip")"
  test "$download_size" = "$artifact_size" || { ARTIFACT_CHECK=invalid; return 1; }
  test "$download_size" -le 1048576 || { ARTIFACT_CHECK=invalid; return 1; }
  actual="$(sha256sum "$root/victim.zip" | cut -d' ' -f1)"
  test "$actual" = "$digest" || { ARTIFACT_CHECK=invalid; return 1; }
  members="$(unzip -Z1 "$root/victim.zip" 2>/dev/null)" || { ARTIFACT_CHECK=invalid; return 1; }
  test "$members" = readiness.json || { ARTIFACT_CHECK=invalid; return 1; }
  private_capture "$root" "$root/readiness.json" 65536 unzip -p \
    "$root/victim.zip" readiness.json 2>/dev/null \
    || { ARTIFACT_CHECK=invalid; return 1; }
  private_json_valid "$root" "$root/readiness.json" 65536 \
    || { ARTIFACT_CHECK=invalid; return 1; }
  if ! jq -e --arg probe "$PROBE_ID" --arg mode "$VICTIM_MODE" --arg phase "$PROBE_PHASE" \
    --arg id "$RUN_ID" --arg repo "$GITHUB_REPOSITORY" --arg sha "$GITHUB_SHA" \
    --arg scope "$CACHE_SCOPE" --arg generation "$MBX_GENERATION" \
    --arg mbx_action "$MBX_ACTION_USES" --arg mbx_version "$MBX_VERSION" \
    --arg rust "$RUST_VERSION" --arg mise_action "$MISE_ACTION_USES" \
    --arg mise_version "$MISE_VERSION" --arg mise_sha "$MISE_SHA256" \
    --arg actor "$(jq -er '.actor.login | strings' "$run_path")" \
    '.schema == 1 and .probe_id == $probe and .mode == $mode and .phase == $phase
     and .child_run_id == $id and .child_attempt == "1" and .repository == $repo
     and .workflow_path == ".github/workflows/qualification.yml"
     and .event == "workflow_dispatch" and .ref == "refs/heads/main" and .source_sha == $sha
     and .actor == $actor and .mbx_action_uses == $mbx_action and .mbx_version == $mbx_version
     and .mbx_resolved_version == $mbx_version and .cache_scope == $scope
     and (.actor | type == "string" and length > 0 and (index("\n") == null) and (index("\r") == null))
     and .generation == $generation and (.rustc_identity | test("^[0-9a-f]{64}$"))
     and .rust_version == $rust and .mise_action_uses == $mise_action
     and .mise_version == $mise_version and .mise_sha256 == $mise_sha
     and (.cache_key | type == "string" and length > 0)' "$root/readiness.json" >/dev/null 2>&1; then
    ARTIFACT_CHECK=invalid
    return 1
  fi
  validate_key || { ARTIFACT_CHECK=invalid; return 1; }
  if [ ! -e "$root/validated-artifact.json" ] && [ ! -L "$root/validated-artifact.json" ]; then
    private_capture "$root" "$root/validated-artifact.json" 1024 jq -cn \
      --arg id "$artifact_id" --arg digest "$digest" --argjson size "$artifact_size" \
      '{id:$id,digest:$digest,size_in_bytes:$size}' \
      || { ARTIFACT_CHECK=invalid; return 1; }
  fi
  private_capture "$root" "$root/validated-victim.json" 65536 jq \
    '{schema,probe_id,mode,phase,child_run_id,child_attempt,repository,workflow_path,event,ref,source_sha,actor,
       mbx_action_uses,mbx_version,mbx_resolved_version,cache_scope,cache_key,generation,rustc_identity,
       rust_version,mise_action_uses,mise_version,mise_sha256}' "$root/readiness.json" \
    || { ARTIFACT_CHECK=invalid; return 1; }
  ARTIFACT_CHECK=valid
  return 0
}
"#;

pub(in crate::schema2::mbx_cancel_probe) fn wait_readiness() -> String {
    format!("{API_COMMON}{RECEIPT_HELPERS}{WAIT_READINESS_BODY}")
}

const CANCEL_EXACT_BODY: &str = r#"
cancel_requested=false
reason=exact_run_or_attempt_changed
cancel_status=not_attempted
post_revalidated=false
if [ -s "$root/validated-victim.json" ] \
  && fetch_run "$root/run.json" && run_valid "$root/run.json" \
  && fetch_jobs "$root/jobs.json" && [ "$(job_count "$root/jobs.json")" = 1 ] \
  && [ "$(job_state "$root/jobs.json")" = in_progress ] \
  && target_live "$root/jobs.json" && receipt_step_complete "$root/jobs.json" \
  && jq -e --arg actor "$(jq -er '.actor' "$root/validated-victim.json")" '.actor.login == $actor' "$root/run.json" >/dev/null 2>&1; then
  cache_key="$(jq -er '.cache_key | strings' "$root/validated-victim.json")"
  if private_gh_json "$root" "$root/cache-before.json" --method GET \
      "/repos/$GITHUB_REPOSITORY/actions/caches?key=$cache_key&ref=refs/heads/main&per_page=100" 2>/dev/null \
      && private_list_complete "$root/cache-before.json" actions_caches; then
    private_capture "$root" "$root/cache-before-exact.json" 65536 \
      cache_snapshot "$root/cache-before.json" "$cache_key"
  else
    private_capture "$root" "$root/cache-before-exact.json" 65536 \
      printf '%s\n' '{"count":-1,"caches":[]}'
  fi
  if fetch_run "$root/run-before-artifact.json" && run_valid "$root/run-before-artifact.json" \
    && jq -e --arg actor "$(jq -er '.actor' "$root/validated-victim.json")" '.actor.login == $actor' "$root/run-before-artifact.json" >/dev/null 2>&1 \
    && validate_victim_artifact "$root/run-before-artifact.json"; then
    if fetch_run "$root/run-before-cancel.json" && run_valid "$root/run-before-cancel.json" \
      && fetch_jobs "$root/jobs-before-cancel.json" \
      && [ "$(job_count "$root/jobs-before-cancel.json")" = 1 ] \
      && [ "$(job_state "$root/jobs-before-cancel.json")" = in_progress ] \
      && target_live "$root/jobs-before-cancel.json" \
      && receipt_step_complete "$root/jobs-before-cancel.json" \
      && jq -e --arg actor "$(jq -er '.actor' "$root/validated-victim.json")" '.actor.login == $actor' "$root/run-before-cancel.json" >/dev/null 2>&1; then
    cancel_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    private_capture "$root" "$root/cancel-request-started-at" 128 printf '%s\n' "$cancel_at"
    private_capture "$root" "$root/cancel-response.txt" 65536 \
      gh_api --include --method POST "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/cancel" \
      2>/dev/null || true
    private_file_valid "$root" "$root/cancel-response.txt" 65536 || exit 0
    cancel_status="$(sed -n '1s/^[^ ]* \([0-9][0-9][0-9]\).*/\1/p' "$root/cancel-response.txt")"
    if [ "$cancel_status" = 202 ]; then
      cancel_requested=true
      reason=exact_child_cancel_accepted
      if fetch_run "$root/run-after-cancel.json" && run_valid "$root/run-after-cancel.json" \
        && jq -e --arg actor "$(jq -er '.actor' "$root/validated-victim.json")" \
          '.actor.login == $actor' "$root/run-after-cancel.json" >/dev/null 2>&1; then
        post_revalidated=true
      fi
    else
      reason=cancel_not_accepted
    fi
    fi
  fi
fi
printf 'cancel_requested=%s\nreason=%s\ncancel_status=%s\npost_revalidated=%s\n' \
  "$cancel_requested" "$reason" "$cancel_status" "$post_revalidated" >> "$GITHUB_OUTPUT"
"#;

const WAIT_TERMINAL_BODY: &str = r#"
terminal=false
terminal_state=unknown
if valid_id "${RUN_ID:-}" && valid_id "${WORKFLOW_ID:-}"; then
  for try in $(seq 1 240); do
    if fetch_run "$root/run.json" && run_valid "$root/run.json"; then
      status="$(jq -er '.status' "$root/run.json")"
      terminal_state="$(jq -er '.status + "/" + (.conclusion // "unknown")' "$root/run.json")"
      if [ "$status" = completed ]; then terminal=true; break; fi
    fi
    sleep 10
  done
fi
printf 'terminal=%s\nterminal_state=%s\n' "$terminal" "$terminal_state" >> "$GITHUB_OUTPUT"
"#;

pub(in crate::schema2::mbx_cancel_probe) fn cancel_exact() -> String {
    format!("{API_COMMON}{RECEIPT_HELPERS}{CACHE_SNAPSHOT_FUNCTION}{CANCEL_EXACT_BODY}")
}

pub(in crate::schema2::mbx_cancel_probe) fn wait_terminal() -> String {
    format!("{API_COMMON}{WAIT_TERMINAL_BODY}")
}
