//! Whitelisted controller receipt production and strict observer validation.

pub(in crate::schema2::mbx_cancel_probe) const CONTROLLER_RECEIPT: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-controller"
path="$RUNNER_TEMP/mbx-cancel/receipt.json"
mkdir -m 700 -p "${path%/*}"
victim=null
before='{"count":-1,"caches":[]}'
cancel_at=
if [ -s "$root/validated-victim.json" ]; then victim="$(jq -c . "$root/validated-victim.json")"; fi
if [ -s "$root/cache-before-exact.json" ]; then before="$(jq -c . "$root/cache-before-exact.json")"; fi
if [ -s "$root/cancel-request-started-at" ]; then IFS= read -r cancel_at < "$root/cancel-request-started-at"; fi
jq -cn \
  --arg probe "$PROBE_ID" --arg phase "$PROBE_PHASE" --arg mode "$CONTROLLER_MODE" \
  --arg repository "$GITHUB_REPOSITORY" --arg source_sha "$GITHUB_SHA" \
  --arg controller_run_id "$GITHUB_RUN_ID" --arg controller_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg controller_actor "$GITHUB_ACTOR" --arg child_run_id "$RUN_ID" \
  --arg child_workflow_id "$WORKFLOW_ID" --arg child_run_url "${RUN_URL:-}" \
  --arg dispatch_status "${DISPATCH_STATUS:-}" --arg ready "$READY" --arg ready_reason "$READY_REASON" \
  --arg cancel_requested "$CANCEL_REQUESTED" --arg cancel_status "$CANCEL_STATUS" \
  --arg cancel_reason "$CANCEL_REASON" --arg post_revalidated "$POST_REVALIDATED" \
  --arg terminal "$TERMINAL" \
  --arg terminal_state "$TERMINAL_STATE" --arg cancel_at "$cancel_at" \
  --argjson victim "$victim" --argjson cache_before "$before" \
  '{schema:1,probe_id:$probe,phase:$phase,controller_mode:$mode,repository:$repository,
    controller_run_id:$controller_run_id,controller_attempt:$controller_attempt,
    controller_source_sha:$source_sha,controller_actor:$controller_actor,
    child_run_id:$child_run_id,child_workflow_id:$child_workflow_id,child_run_url:$child_run_url,
    dispatch_status:$dispatch_status,ready:($ready == "true"),ready_reason:$ready_reason,
    cancel_requested:($cancel_requested == "true"),cancel_status:$cancel_status,
    cancel_reason:$cancel_reason,post_revalidated:($post_revalidated == "true"),
    cancel_request_started_at:$cancel_at,
    terminal:($terminal == "true"),terminal_state:$terminal_state,
    cache_before:$cache_before,victim:$victim}' > "$path"
chmod 600 "$path"
"#;

pub(in crate::schema2::mbx_cancel_probe) const VALIDATE_CONTROLLER_RECEIPT: &str = r#"set -euo pipefail
gh_api() { gh api --hostname github.com "$@"; }
path="$RUNNER_TEMP/mbx-cancel/controller/receipt.json"
should_observe=false
trap 'printf "should_observe=%s\\n" "$should_observe" >> "$GITHUB_OUTPUT"' EXIT
test "$GITHUB_REPOSITORY" = tailrocks/velnor-new || exit 0
test "$GITHUB_EVENT_NAME" = workflow_dispatch || exit 0
test "$GITHUB_REF" = refs/heads/main || exit 0
test "$REF_PROTECTED" = true || exit 0
test "$GITHUB_WORKFLOW_REF" = "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main" || exit 0
test -s "$path" || exit 0
jq -e --arg expected_scope "$CACHE_SCOPE" --arg phase "$PROBE_PHASE" --arg mode "$CONTROLLER_MODE" \
  --arg repo "$GITHUB_REPOSITORY" --arg run "$GITHUB_RUN_ID" \
  --arg attempt "$GITHUB_RUN_ATTEMPT" --arg sha "$GITHUB_SHA" \
  --arg workflow_path .github/workflows/qualification.yml \
  --arg mbx_action "$MBX_ACTION_USES" --arg mbx_version "$MBX_VERSION" \
  --arg generation "$MBX_GENERATION" --arg rust_version "$RUST_VERSION" \
  --arg mise_action "$MISE_ACTION_USES" --arg mise_version "$MISE_VERSION" \
  --arg mise_sha "$MISE_SHA256" \
  '. as $receipt | $receipt.schema == 1 and $receipt.phase == $phase and $receipt.controller_mode == $mode
   and .repository == $repo and .controller_run_id == $run
   and .controller_attempt == $attempt and .controller_source_sha == $sha
   and ($receipt.controller_actor | type == "string" and test("^[A-Za-z0-9][A-Za-z0-9-]{0,38}(\\[bot\\])?$"))
   and (.probe_id | type == "string" and test("^[0-9a-f]{32}$"))
   and .dispatch_status == "200" and (.child_run_id | test("^[1-9][0-9]*$"))
   and (.child_workflow_id | test("^[1-9][0-9]*$"))
   and .child_run_url == ("https://api.github.com/repos/" + $repo + "/actions/runs/" + .child_run_id)
   and .victim.child_run_id == .child_run_id and .victim.child_attempt == "1"
   and .victim.probe_id == .probe_id and .victim.phase == $phase
   and .victim.mode == (if $phase == "pre-save" then "mbx-cancel-pre-save-victim" else "mbx-cancel-during-save-victim" end)
   and .victim.repository == $repo and .victim.source_sha == $sha
   and .victim.event == "workflow_dispatch" and .victim.ref == "refs/heads/main"
   and .victim.cache_scope == $expected_scope
   and .victim.workflow_path == $workflow_path
   and .victim.mbx_action_uses == $mbx_action
   and .victim.mbx_version == $mbx_version and .victim.mbx_resolved_version == $mbx_version
   and .victim.generation == $generation
   and .victim.rust_version == $rust_version
   and .victim.mise_action_uses == $mise_action and .victim.mise_version == $mise_version
   and .victim.mise_sha256 == $mise_sha
   and (.victim.actor | type == "string" and length <= 44
     and test("^[A-Za-z0-9][A-Za-z0-9-]{0,38}(\\[bot\\])?$"))
   and (.victim.child_run_id | type == "string" and test("^[1-9][0-9]{0,19}$")
     and (index("\n") == null) and (index("\r") == null))
   and (.victim.child_attempt | type == "string" and . == "1")
   and (.victim.source_sha | type == "string" and test("^[0-9a-f]{40}$")
     and (index("\n") == null) and (index("\r") == null))
   and (.victim.cache_scope | type == "string" and (index("\n") == null) and (index("\r") == null))
   and (.victim.generation | type == "string" and (index("\n") == null) and (index("\r") == null))
   and (.victim.mbx_version | type == "string" and (index("\n") == null) and (index("\r") == null))
   and (.victim.rustc_identity | test("^[0-9a-f]{64}$"))
   and (.victim.cache_key | type == "string" and length <= 512
     and (index("\n") == null) and (index("\r") == null))
   and (.ready | type == "boolean")
   and (.ready_reason | type == "string" and test("^[a-z0-9_-]+$"))
   and (.cancel_requested | type == "boolean")
   and (.cancel_status | type == "string" and test("^(not_attempted|[0-9]{3})$"))
   and (.cancel_reason | type == "string" and test("^[a-z0-9_-]+$"))
   and (.post_revalidated | type == "boolean")
   and (.terminal | type == "boolean")
   and (.terminal_state | type == "string" and test("^[a-z_]+/[a-z_]+$"))
   and (.cancel_request_started_at | type == "string"
     and test("^(|[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z)$"))
   and ($receipt.cache_before.count | type == "number" and . >= -1 and . == floor)
   and ($receipt.cache_before.caches | type == "array")
   and (($receipt.cache_before.count == -1 and $receipt.cache_before.caches == [])
     or ($receipt.cache_before.count >= 0
       and ($receipt.cache_before.caches | length) == $receipt.cache_before.count))' \
  "$path" >/dev/null 2>&1 || exit 0
jq -e --arg mode "$CONTROLLER_MODE" \
  '.inputs.mode == $mode and (.inputs.probe_id | type == "string")' \
  "$GITHUB_EVENT_PATH" >/dev/null 2>&1 || exit 0
child_id="$(jq -er '.child_run_id' "$path" 2>/dev/null || true)"
child_attempt="$(jq -er '.victim.child_attempt' "$path" 2>/dev/null || true)"
child_sha="$(jq -er '.victim.source_sha' "$path" 2>/dev/null || true)"
rustc_identity="$(jq -er '.victim.rustc_identity' "$path" 2>/dev/null || true)"
receipt_key="$(jq -er '.victim.cache_key' "$path" 2>/dev/null || true)"
case "$child_id:$child_attempt:$child_sha:$rustc_identity" in
  *[!0-9a-f:]*) exit 0 ;;
esac
[[ "$child_id" =~ ^[1-9][0-9]{0,19}$ ]] || exit 0
test "$child_attempt" = 1 || exit 0
[[ "$child_sha" =~ ^[0-9a-f]{40}$ ]] || exit 0
[[ "$rustc_identity" =~ ^[0-9a-f]{64}$ ]] || exit 0
test "$MBX_GENERATION" = "velnor-mbx-$MBX_VERSION" || exit 0
scope_hash="$(printf '%s\n%s\n%s\n' \
  "$GITHUB_REPOSITORY/.github/workflows/qualification.yml" "$CACHE_SCOPE" '{}' \
  | sha256sum | cut -c1-64)" || exit 0
expected_key="linux-x64-mbx-$MBX_GENERATION-dir-rust-$RUST_VERSION-$rustc_identity-scope-$scope_hash-run-$child_id-attempt-$child_attempt-$child_sha"
test "$receipt_key" = "$expected_key" || exit 0
root="$RUNNER_TEMP/mbx-cancel/observer"
mkdir -m 700 -p "$root"
gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID" > "$root/controller.json" 2>/dev/null || exit 0
jq -e --arg run "$GITHUB_RUN_ID" --arg sha "$GITHUB_SHA" \
  --arg workflow "$(jq -er '.child_workflow_id' "$path")" \
  --arg actor "$(jq -er '.controller_actor' "$path")" \
   '.id == ($run | tonumber) and .workflow_id == ($workflow | tonumber)
   and .run_attempt == 1 and .head_sha == $sha
   and .head_branch == "main" and .event == "workflow_dispatch"
   and .repository.full_name == "tailrocks/velnor-new"
   and .head_repository.full_name == "tailrocks/velnor-new" and .actor.login == $actor
   and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
   and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
  "$root/controller.json" >/dev/null 2>&1 || exit 0
child="$(jq -er '.child_run_id' "$path")"
workflow="$(jq -er '.child_workflow_id' "$path")"
if ! gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$child" > "$root/child.json" 2>/dev/null; then exit 0; fi
jq -e --arg id "$child" --arg workflow "$workflow" --arg repo "$GITHUB_REPOSITORY" \
  --arg sha "$GITHUB_SHA" --arg probe "$(jq -er '.probe_id' "$path")" \
  --arg actor "$(jq -er '.victim.actor' "$path")" \
  --arg mode "$(jq -er '.victim.mode' "$path")" \
  '.id == ($id | tonumber) and .workflow_id == ($workflow | tonumber)
   and .repository.full_name == $repo and .head_repository.full_name == $repo
   and .head_sha == $sha and .head_branch == "main" and .run_attempt == 1
   and .event == "workflow_dispatch" and .actor.login == $actor
   and .display_title == ("MBX cancellation " + $mode + " " + $probe)
   and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
   and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
  "$root/child.json" >/dev/null 2>&1 || exit 0
jq -r '.victim | {probe_id,child_run_id,child_attempt,source_sha,cache_key,cache_scope,
      generation,rustc_identity,mbx_version,mode,phase,actor,repository,workflow_path,
      mbx_action_uses,mbx_resolved_version,rust_version,mise_action_uses,mise_version,mise_sha256}' \
  "$path" > "$root/validated-victim.json"
{
  jq -r '. as $receipt | $receipt.victim |
    "probe_id=\(.probe_id)\nchild_run_id=\(.child_run_id)\nchild_attempt=\(.child_attempt)\nsource_sha=\(.source_sha)\ncache_key=\(.cache_key)\ncache_scope=\(.cache_scope)\ngeneration=\(.generation)\nrustc_identity=\(.rustc_identity)\nmbx_version=\(.mbx_version)\nchild_workflow_id=\($receipt.child_workflow_id)\nchild_actor=\(.actor)\nready=\($receipt.ready)\nready_reason=\($receipt.ready_reason)\ncancel_requested=\($receipt.cancel_requested)\ncancel_status=\($receipt.cancel_status)\npost_revalidated=\($receipt.post_revalidated)\ncancel_request_started_at=\($receipt.cancel_request_started_at)\nterminal=\($receipt.terminal)\nterminal_state=\($receipt.terminal_state)\ncontroller_cache_before_count=\($receipt.cache_before.count)"' "$path"
} >> "$GITHUB_OUTPUT"
should_observe=true
"#;
