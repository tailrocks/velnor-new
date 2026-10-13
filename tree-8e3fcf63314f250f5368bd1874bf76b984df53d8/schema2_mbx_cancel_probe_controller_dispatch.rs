//! Exact workflow dispatch and initial returned-run binding.

pub(in crate::schema2::mbx_cancel_probe) const GENERATE_ID: &str = r#"set -euo pipefail
private_root_create "$RUNNER_TEMP/mbx-cancel-controller"
probe_id="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
[[ "$probe_id" =~ ^[0-9a-f]{32}$ ]]
printf 'probe_id=%s\n' "$probe_id" >> "$GITHUB_OUTPUT"
"#;

pub(in crate::schema2::mbx_cancel_probe) const DISPATCH: &str = r#"set -euo pipefail
gh_api() { gh api --hostname github.com "$@"; }
dispatch_status=not_sent
workflow_id=
run_id=
run_url=
candidate_workflow_id=
candidate_run_id=
candidate_run_url=
record_dispatch() {
  printf 'dispatch_status=%s\nworkflow_id=%s\nworkflow_run_id=%s\nrun_url=%s\n' \
    "$dispatch_status" "$workflow_id" "$run_id" "$run_url" >> "$GITHUB_OUTPUT"
}
trap record_dispatch EXIT
test "$GITHUB_REPOSITORY" = tailrocks/velnor-new
test "$GITHUB_EVENT_NAME" = workflow_dispatch
test "$GITHUB_REF" = refs/heads/main
test "$REF_PROTECTED" = true
case "$PROBE_ID" in ''|*[!0-9a-f]*) exit 1 ;; esac
[[ "$PROBE_ID" =~ ^[0-9a-f]{32}$ ]]
case "$GITHUB_SHA" in ''|*[!0-9a-f]*) exit 1 ;; esac
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
case "$VICTIM_MODE" in
  mbx-cancel-pre-save-victim|mbx-cancel-during-save-victim) ;;
  *) exit 1 ;;
esac
root="$RUNNER_TEMP/mbx-cancel-controller"
private_root_open "$root"
if ! private_gh_json "$root" "$root/workflow.json" --method GET \
    "/repos/$GITHUB_REPOSITORY/actions/workflows/qualification.yml" 2>/dev/null; then exit 0; fi
candidate_workflow_id="$(jq -er '.id | select(type == "number" and . > 0 and . <= 9007199254740991 and . == floor)' "$root/workflow.json" 2>/dev/null || true)"
test -n "$candidate_workflow_id" || exit 0
jq -e '.path == ".github/workflows/qualification.yml" and .state == "active"' "$root/workflow.json" >/dev/null 2>&1 || { workflow_id=; exit 0; }
private_capture "$root" "$root/dispatch.json" 65536 jq -cn \
  --arg ref refs/heads/main --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
  '{ref:$ref,return_run_details:true,inputs:{mode:$mode,probe_id:$probe}}'
private_capture "$root" "$root/dispatch-response.txt" 2097152 gh_api --include --method POST \
  --input "$root/dispatch.json" \
  "/repos/$GITHUB_REPOSITORY/actions/workflows/qualification.yml/dispatches" 2>/dev/null || true
private_file_valid "$root" "$root/dispatch-response.txt" 2097152 || exit 0
status="$(sed -n '1s/^[^ ]* \([0-9][0-9][0-9]\).*/\1/p' "$root/dispatch-response.txt")"
dispatch_status="${status:-request_failed}"
test "$status" = 200 || exit 0
private_capture "$root" "$root/dispatch-response.json" 65536 awk \
  'BEGIN { body=0 } { sub(/\r$/, ""); if (!body && $0 == "") { body=1; next } if (body) print }' \
  "$root/dispatch-response.txt"
private_json_valid "$root" "$root/dispatch-response.json" 65536 || exit 0
candidate_run_id="$(jq -er '.workflow_run_id | select(type == "number" and . > 0 and . <= 9007199254740991 and . == floor)' "$root/dispatch-response.json" 2>/dev/null || true)"
test -n "$candidate_run_id" || exit 0
expected_url="https://api.github.com/repos/$GITHUB_REPOSITORY/actions/runs/$candidate_run_id"
candidate_run_url="$(jq -er '.run_url | strings' "$root/dispatch-response.json" 2>/dev/null || true)"
test "$candidate_run_url" = "$expected_url" || exit 0
for attempt in 1 2 3 4 5 6; do
  if private_gh_json "$root" "$root/run.json" --method GET \
      "/repos/$GITHUB_REPOSITORY/actions/runs/$candidate_run_id" 2>/dev/null; then
    break
  fi
  sleep 2
done
jq -e --argjson id "$candidate_run_id" --argjson workflow "$candidate_workflow_id" \
  --arg repo "$GITHUB_REPOSITORY" --arg sha "$GITHUB_SHA" \
  --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
  '.id == $id and .workflow_id == $workflow and .repository.full_name == $repo
   and .head_repository.full_name == $repo
   and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
   and .run_attempt == 1 and .display_title == ("MBX cancellation " + $mode + " " + $probe)
   and (.actor.login | type == "string" and length <= 44
     and test("^[A-Za-z0-9][A-Za-z0-9-]{0,38}(\\[bot\\])?$"))
   and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
   and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
  "$root/run.json" >/dev/null 2>&1 || exit 0
workflow_id="$candidate_workflow_id"
run_id="$candidate_run_id"
run_url="$candidate_run_url"
"#;
