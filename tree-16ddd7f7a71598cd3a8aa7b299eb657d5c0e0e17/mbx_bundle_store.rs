//! Private MBX store scripts and lane ownership.

/// Released action v1.7.1 owns the hosted store when this input is present.
pub(super) const HOSTED_STORE_ISOLATION: &str = "${{ runner.environment == 'github-hosted' }}";
/// Velnor owns the manual bundle store only on Scale Set with action isolation.
pub(super) const SCALE_SET_ONLY_IF: &str = "runner.environment != 'github-hosted'";

/// Initialize a mode-700 cache root in this job's temporary directory.
pub(super) const STORE_INIT_SCRIPT: &str = r#"set -eu
umask 077
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_ENV:?GITHUB_ENV is required}"
: "${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}"
: "${GITHUB_RUN_ATTEMPT:?GITHUB_RUN_ATTEMPT is required}"
: "${GITHUB_JOB:?GITHUB_JOB is required}"
case "$GITHUB_RUN_ID" in ''|*[!0-9]*) exit 1 ;; esac
case "$GITHUB_RUN_ATTEMPT" in ''|*[!0-9]*) exit 1 ;; esac
case "$GITHUB_JOB" in ''|*[!A-Za-z0-9_-]*) exit 1 ;; esac
temp_root=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P)
root=$(mktemp -d "$temp_root/velnor-mbx-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT.XXXXXXXXXX")
test -d "$root" && test ! -L "$root"
root=$(CDPATH= cd -- "$root" && pwd -P)
chmod 700 "$root"
marker="$root/.velnor-mbx-owner"
if ! (set -C; printf 'run_id=%s\njob=%s\nattempt=%s\n' "$GITHUB_RUN_ID" "$GITHUB_JOB" "$GITHUB_RUN_ATTEMPT" > "$marker"); then
    echo "could not create private MBX owner marker" >&2
    exit 1
fi
printf 'MBX_CACHE_DIR=%s\n' "$root" >> "$GITHUB_ENV"
printf 'Initialized private MBX store under runner.temp.\n'"#;

/// Validate exact private ownership, export one bundle, and preserve the store.
pub(super) const EXPORT_SCRIPT: &str = r#"set -u
cache_unavailable() {
    reason="$1"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf 'ready=false\nacceptance=cache_unavailable\n' >> "$GITHUB_OUTPUT" || true
    fi
    printf '::warning::MBX cache unavailable: %s\n' "$reason" >&2
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        printf 'MBX cache acceptance: cache_unavailable (%s)\n' "$reason" >> "$GITHUB_STEP_SUMMARY" || true
    fi
    exit 0
}
no_entry() {
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf 'ready=false\nacceptance=no_entry\n' >> "$GITHUB_OUTPUT" || true
    fi
    printf 'MBX cache acceptance: no_entry\n' >> "${GITHUB_STEP_SUMMARY:-/dev/null}" || true
    exit 0
}
if [ "${MBX_CACHE_IMPORT_UNAVAILABLE:-}" = "true" ]; then
    cache_unavailable "prior-bundle-import-uncertain"
fi
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
bundle="$RUNNER_TEMP/mbx-single-bundle-export"
if [ -e "$bundle" ] || [ -L "$bundle" ]; then
    cache_unavailable "bundle-path-already-exists"
fi
if ! df -B1 -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
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
    if ! df -B1 -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
        cache_unavailable "disk-measurement-failed-after-export"
    fi
    printf 'ready=true\nacceptance=accepted\ncleanup=runner-temp\n' >> "$GITHUB_OUTPUT" || true
    printf 'MBX cache acceptance: accepted; private store cleanup belongs to runner.temp.\n' >> "${GITHUB_STEP_SUMMARY:-/dev/null}" || true
else
    if grep -Fq 'no completed mbx builds are recorded for export group' "$RUNNER_TEMP/mbx-export.out"; then
        no_entry
    fi
    cat "$RUNNER_TEMP/mbx-export.out" >&2 || true
    cache_unavailable "bundle-export-failed"
fi"#;
