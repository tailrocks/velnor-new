//! Victim identity, natural build work, and readiness receipt scripts.

pub(in crate::schema2::mbx_cancel_probe) const VICTIM_IDENTITY: &str = r#"set -euo pipefail
test "$GITHUB_REPOSITORY" = "tailrocks/velnor-new"
test "$GITHUB_EVENT_NAME" = workflow_dispatch
test "$GITHUB_REF" = refs/heads/main
test "$REF_PROTECTED" = true
test "$GITHUB_RUN_ATTEMPT" = 1
test "$GITHUB_WORKFLOW_REF" = "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main"
[[ "$PROBE_ID" =~ ^[0-9a-f]{32}$ ]]
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
jq -e --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
  '.inputs.mode == $mode and .inputs.probe_id == $probe' "$GITHUB_EVENT_PATH" >/dev/null
[[ "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ ]]
test "$GITHUB_RUN_ATTEMPT" = 1
"#;

pub(in crate::schema2::mbx_cancel_probe) const FETCH_SOURCE: &str = r#"set -euo pipefail
repo="$RUNNER_TEMP/mbx-cancel-source"
test ! -e "$repo" && test ! -L "$repo"
git init --quiet "$repo"
git -C "$repo" remote add origin "https://github.com/$GITHUB_REPOSITORY.git"
git -C "$repo" fetch --quiet --depth=1 --no-tags origin "$GITHUB_SHA"
git -C "$repo" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$repo" rev-parse HEAD)" = "$GITHUB_SHA"
"#;

pub(in crate::schema2::mbx_cancel_probe) const BUILD_WORKSPACE: &str = r#"set -euo pipefail
repo="$RUNNER_TEMP/mbx-cancel-source"
test "$(git -C "$repo" rev-parse HEAD)" = "$GITHUB_SHA"
cd "$repo"
mbx build --locked --workspace
"#;

pub(in crate::schema2::mbx_cancel_probe) const PRE_SAVE_GUARD: &str = r#"set -euo pipefail
test "$PROBE_PHASE" = pre-save
test -f "$RUNNER_TEMP/mbx-cancel/victim/readiness.json"
"#;

pub(in crate::schema2::mbx_cancel_probe) const PRE_SAVE_WAIT: &str = r#"set -euo pipefail
test "$PROBE_PHASE" = pre-save
sleep 600
"#;

pub(in crate::schema2::mbx_cancel_probe) const WRITE_VICTIM_RECEIPT: &str = r#"set -euo pipefail
case "$MBX_RUSTC_IDENTITY" in ''|*[!0-9a-f]*) exit 1 ;; esac
[[ "$MBX_RUSTC_IDENTITY" =~ ^[0-9a-f]{64}$ ]]
test "$MBX_GENERATION" != ""
test "$MBX_VERSION" = "$MBX_RESOLVED_VERSION"
case "$MBX_PRIMARY" in
  linux-x64-mbx-"$MBX_GENERATION"-dir-rust-"$RUST_VERSION"-"$MBX_RUSTC_IDENTITY"-scope-*-run-"$GITHUB_RUN_ID"-attempt-1-"$GITHUB_SHA") ;;
  *) echo 'invalid exact MBX qualification key' >&2; exit 1 ;;
esac
path="$RUNNER_TEMP/mbx-cancel/victim"
test ! -e "$path" && test ! -L "$path"
mkdir -m 700 -p "$path"
jq -cn \
  --arg probe_id "$PROBE_ID" --arg mode "$VICTIM_MODE" --arg phase "$PROBE_PHASE" \
  --arg child_run_id "$GITHUB_RUN_ID" --arg child_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg repository "$GITHUB_REPOSITORY" --arg workflow_path .github/workflows/qualification.yml \
  --arg event "$GITHUB_EVENT_NAME" --arg ref "$GITHUB_REF" --arg source_sha "$GITHUB_SHA" \
  --arg actor "$GITHUB_ACTOR" --arg mbx_action_uses "$MBX_ACTION_USES" \
  --arg mbx_version "$MBX_VERSION" --arg mbx_resolved_version "$MBX_RESOLVED_VERSION" \
  --arg cache_scope "$CACHE_SCOPE" --arg cache_key "$MBX_PRIMARY" \
  --arg generation "$MBX_GENERATION" --arg rustc_identity "$MBX_RUSTC_IDENTITY" \
  --arg rust_version "$RUST_VERSION" --arg mise_action_uses "$MISE_ACTION_USES" \
  --arg mise_version "$MISE_VERSION" --arg mise_sha256 "$MISE_SHA256" \
  '{schema:1,probe_id:$probe_id,mode:$mode,phase:$phase,child_run_id:$child_run_id,
    child_attempt:$child_attempt,repository:$repository,workflow_path:$workflow_path,
    event:$event,ref:$ref,source_sha:$source_sha,actor:$actor,
    mbx_action_uses:$mbx_action_uses,mbx_version:$mbx_version,
    mbx_resolved_version:$mbx_resolved_version,cache_scope:$cache_scope,
    cache_key:$cache_key,generation:$generation,rustc_identity:$rustc_identity,
    rust_version:$rust_version,mise_action_uses:$mise_action_uses,
    mise_version:$mise_version,mise_sha256:$mise_sha256}' \
  > "$path/readiness.json"
chmod 600 "$path/readiness.json"
"#;
