//! Private MBX store scripts and lane ownership.

/// Velnor owns the manual bundle store only on Scale Set.
pub(super) const SCALE_SET_ONLY_IF: &str = "runner.environment != 'github-hosted'";

/// Initialize a mode-700 cache root in this job's temporary directory.
pub(super) const STORE_INIT_SCRIPT: &str = r#"set -eu
umask 077
cache_unavailable() {
    reason="$1"
    printf '::warning::MBX cache unavailable: %s\n' "$reason" >&2
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf 'ready=false\nacceptance=cache_unavailable\n' >> "$GITHUB_OUTPUT" 2>/dev/null || true
    fi
    exit 0
}
if [ -z "${RUNNER_TEMP:-}" ] || [ -z "${GITHUB_ENV:-}" ] || [ -z "${GITHUB_OUTPUT:-}" ]; then
    cache_unavailable "runtime-path-missing"
fi
if [ -z "${GITHUB_RUN_ID:-}" ] || [ -z "${GITHUB_RUN_ATTEMPT:-}" ] || [ -z "${GITHUB_JOB:-}" ]; then
    cache_unavailable "run-identity-missing"
fi
matrix_key=${MBX_MATRIX_KEY:-}
case "$GITHUB_RUN_ID" in ''|*[!0-9]*) cache_unavailable "invalid-run-id" ;; esac
case "$GITHUB_RUN_ATTEMPT" in ''|*[!0-9]*) cache_unavailable "invalid-run-attempt" ;; esac
case "$GITHUB_JOB" in ''|*[!A-Za-z0-9_-]*) cache_unavailable "invalid-job-id" ;; esac
case "$matrix_key" in
    '') matrix_id=nonmatrix ;;
    m-????????????????)
        matrix_digest=${matrix_key#m-}
        case "$matrix_digest" in *[!a-f0-9]*) cache_unavailable "invalid-matrix-id" ;; esac
        matrix_id="$matrix_key"
        ;;
    *) cache_unavailable "invalid-matrix-id" ;;
esac
export_group="velnor-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}-${matrix_id}"
case "$export_group" in ''|*[!A-Za-z0-9_-]*) cache_unavailable "invalid-export-group" ;; esac
if ! temp_root=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P); then
    cache_unavailable "runner-temp-unavailable"
fi
if ! root=$(mktemp -d "$temp_root/velnor-mbx-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT.XXXXXXXXXX"); then
    cache_unavailable "private-root-create-failed"
fi
if [ ! -d "$root" ] || [ -L "$root" ]; then
    cache_unavailable "private-root-identity-failed"
fi
if ! root=$(CDPATH= cd -- "$root" && pwd -P) || ! chmod 700 "$root"; then
    cache_unavailable "private-root-permission-failed"
fi
marker="$root/.velnor-mbx-owner"
if ! (set -C; printf 'run_id=%s\njob=%s\nattempt=%s\n' "$GITHUB_RUN_ID" "$GITHUB_JOB" "$GITHUB_RUN_ATTEMPT" > "$marker"); then
    cache_unavailable "owner-marker-create-failed"
fi
if ! printf 'MBX_CACHE_DIR=%s\nMBX_CACHE_EXPORT_GROUP=%s\n' "$root" "$export_group" >> "$GITHUB_ENV"; then
    cache_unavailable "github-env-handoff-failed"
fi
if ! printf 'ready=true\nacceptance=ready\n' >> "$GITHUB_OUTPUT"; then
    cache_unavailable "github-output-handoff-failed"
fi
printf 'Initialized private MBX store under runner.temp.\n'"#;

/// Validate exact private ownership, export one bundle, and preserve the store.
pub(super) const EXPORT_SCRIPT: &str = r#"set -u
cache_unavailable() {
    reason="$1"
    [ -n "${GITHUB_OUTPUT:-}" ] || { echo "GITHUB_OUTPUT is missing" >&2; exit 1; }
    printf 'ready=false\nacceptance=cache_unavailable\n' >> "$GITHUB_OUTPUT" || exit 1
    printf '::warning::MBX cache unavailable: %s\n' "$reason" >&2
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        printf 'MBX cache acceptance: cache_unavailable (%s)\n' "$reason" >> "$GITHUB_STEP_SUMMARY" || true
    fi
    exit 0
}
no_entry() {
    [ -n "${GITHUB_OUTPUT:-}" ] || { echo "GITHUB_OUTPUT is missing" >&2; exit 1; }
    printf 'ready=false\nacceptance=no_entry\n' >> "$GITHUB_OUTPUT" || exit 1
    printf 'MBX cache acceptance: no_entry\n' >> "${GITHUB_STEP_SUMMARY:-/dev/null}" || true
    exit 0
}
case "${MBX_CACHE_IMPORT_STATE:-}" in
    cold|imported) ;;
    *) cache_unavailable "import-outcome-missing-or-uncertain" ;;
esac
run_id=${GITHUB_RUN_ID:-}
run_attempt=${GITHUB_RUN_ATTEMPT:-}
job_id=${GITHUB_JOB:-}
case "$run_id" in ''|*[!0-9]*) cache_unavailable "invalid-run-identity" ;; esac
case "$run_attempt" in ''|*[!0-9]*) cache_unavailable "invalid-run-identity" ;; esac
case "$job_id" in ''|*[!A-Za-z0-9_-]*) cache_unavailable "invalid-run-identity" ;; esac
if [ -z "${RUNNER_TEMP:-}" ] || [ -z "${MBX_CACHE_DIR:-}" ] || [ -z "${GITHUB_OUTPUT:-}" ]; then
    cache_unavailable "missing-runtime-path"
fi
if ! temp_root=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P); then
    cache_unavailable "runner-temp-unavailable"
fi
expected_prefix="$temp_root/velnor-mbx-${run_id}-${run_attempt}."
case "$MBX_CACHE_DIR" in "$expected_prefix"*) ;; *) cache_unavailable "store-outside-owned-run-root" ;; esac
suffix=${MBX_CACHE_DIR#"$expected_prefix"}
case "$suffix" in ''|*[!A-Za-z0-9]*) cache_unavailable "invalid-store-path" ;; esac
if [ ! -d "$MBX_CACHE_DIR" ] || [ -L "$MBX_CACHE_DIR" ]; then
    cache_unavailable "store-root-missing-or-symlink"
fi
if ! root_real=$(CDPATH= cd -- "$MBX_CACHE_DIR" && pwd -P) || [ "$root_real" != "$MBX_CACHE_DIR" ]; then
    cache_unavailable "store-root-not-canonical"
fi
marker="$MBX_CACHE_DIR/.velnor-mbx-owner"
if [ ! -f "$marker" ] || [ -L "$marker" ]; then
    cache_unavailable "owner-marker-missing-or-symlink"
fi
expected_marker=$(printf 'run_id=%s\njob=%s\nattempt=%s\n.' "$run_id" "$job_id" "$run_attempt")
expected_marker=${expected_marker%.}
if ! actual_marker=$(cat "$marker" && printf '.'); then
    cache_unavailable "owner-marker-unreadable"
fi
actual_marker=${actual_marker%.}
if [ "$actual_marker" != "$expected_marker" ]; then
    cache_unavailable "owner-marker-mismatch"
fi
expected_store="$MBX_CACHE_DIR/actions"
if ! store=$(mbx cache dir 2>"$RUNNER_TEMP/mbx-store-path.err") || [ "$store" != "$expected_store" ]; then
    cache_unavailable "mbx-store-path-mismatch"
fi
if [ ! -d "$expected_store" ] || [ -L "$expected_store" ]; then
    cache_unavailable "mbx-store-missing-or-symlink"
fi
if ! store_real=$(CDPATH= cd -- "$expected_store" && pwd -P) || [ "$store_real" != "$expected_store" ]; then
    cache_unavailable "mbx-store-not-canonical"
fi
bundle="$RUNNER_TEMP/mbx-single-bundle"
if [ -e "$bundle" ] || [ -L "$bundle" ]; then
    cache_unavailable "bundle-path-already-exists"
fi
if ! df -k -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
    cache_unavailable "disk-measurement-failed-before-export"
fi
if [ -z "${MBX_CACHE_EXPORT_GROUP:-}" ]; then
    cache_unavailable "missing-export-group"
fi
if ! mbx gc >"$RUNNER_TEMP/mbx-gc.out" 2>&1; then
    cat "$RUNNER_TEMP/mbx-gc.out" >&2 || true
    cache_unavailable "garbage-collection-failed"
fi
if mbx cache export --group "$MBX_CACHE_EXPORT_GROUP" --format directory "$bundle" >"$RUNNER_TEMP/mbx-export.out" 2>&1; then
    if [ ! -d "$bundle" ] || [ -L "$bundle" ]; then
        cache_unavailable "export-output-missing-or-symlink"
    fi
    if ! df -k -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
        cache_unavailable "disk-measurement-failed-after-export"
    fi
    printf 'ready=true\nacceptance=accepted\ncleanup=runner-temp\n' >> "$GITHUB_OUTPUT" || exit 1
    printf 'MBX cache acceptance: accepted; private store cleanup belongs to runner.temp.\n' >> "${GITHUB_STEP_SUMMARY:-/dev/null}" || true
else
    if grep -Fq 'no completed mbx builds are recorded for export group' "$RUNNER_TEMP/mbx-export.out"; then
        no_entry
    fi
    cat "$RUNNER_TEMP/mbx-export.out" >&2 || true
    cache_unavailable "bundle-export-failed"
fi"#;
