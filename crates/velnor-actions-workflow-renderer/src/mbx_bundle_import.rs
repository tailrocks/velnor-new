//! Import a restored MBX bundle without replacing uncertain cache content.

/// Publish a cold/imported result before the Scale Set exporter may run.
pub(super) const IMPORT_SCRIPT: &str = r#"set -eu

bundle="${RUNNER_TEMP:-}/mbx-single-bundle"
staging="${RUNNER_TEMP:-}/mbx-single-bundle-imported"

publish_state() {
    state="$1"
    acceptance="$2"
    reason="$3"
    if [ -z "${GITHUB_ENV:-}" ] || ! printf 'MBX_CACHE_IMPORT_STATE=%s\n' "$state" >> "$GITHUB_ENV"; then
        state=unavailable
        acceptance=cache_unavailable
        reason=github-env-handoff-failed
    fi
    if [ -z "${GITHUB_OUTPUT:-}" ] || ! printf \
        'ready=%s\nacceptance=%s\ncache-state=%s\nreason=%s\n' \
        "$([ "$state" = unavailable ] && printf false || printf true)" \
        "$acceptance" "$state" "$reason" >> "$GITHUB_OUTPUT"; then
        printf '::warning::MBX cache outcome handoff failed; cache export is skipped.\n' >&2
        return 1
    fi
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        printf 'MBX cache import: %s (%s)\n' "$acceptance" "$reason" \
            >> "$GITHUB_STEP_SUMMARY" || true
    fi
    if [ "$state" = unavailable ]; then
        printf '::warning::MBX cache acceptance failed: %s\n' "$reason" >&2
    fi
}

cache_unavailable() {
    reason="$1"
    publish_state unavailable cache_unavailable "$reason"
    exit 0
}

cache_corrupt() {
    reason="$1"
    publish_state unavailable cache_corrupt "$reason"
    exit 0
}

if [ -z "${RUNNER_TEMP:-}" ]; then
    cache_unavailable "runner-temp-missing"
fi
if ! temp_root=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P); then
    cache_unavailable "runner-temp-unavailable"
fi
bundle="$temp_root/mbx-single-bundle"
staging="$temp_root/mbx-single-bundle-imported"
expected_bundle="$temp_root/mbx-single-bundle"
if ! df -k -P "$RUNNER_TEMP" || ! df -i -P "$RUNNER_TEMP"; then
    cache_unavailable "disk-measurement-failed"
fi
if [ -z "${MATCHED:-}" ]; then
    if [ -e "$bundle" ] || [ -L "$bundle" ]; then
        cache_unavailable "unmatched-bundle-path-already-exists"
    fi
    publish_state cold no_entry no-matched-bundle
    exit 0
fi
if [ -L "$bundle" ]; then
    cache_unavailable "matched-bundle-is-symlink"
fi
if [ ! -d "$bundle" ]; then
    cache_corrupt "matched-bundle-missing"
fi
if ! bundle_real=$(CDPATH= cd -- "$bundle" && pwd -P) || [ "$bundle_real" != "$expected_bundle" ]; then
    cache_unavailable "matched-bundle-path-mismatch"
fi
if [ -e "$staging" ] || [ -L "$staging" ]; then
    cache_unavailable "import-staging-path-already-exists"
fi
if ! mbx cache import "$bundle"; then
    printf 'mbx bundle import failed; continuing cold\n' >&2
    cache_corrupt "bundle-import-failed"
fi
if ! mv "$bundle" "$staging"; then
    cache_unavailable "imported-bundle-retire-failed"
fi
if [ -e "$bundle" ] || [ -L "$bundle" ] || [ ! -d "$staging" ] || [ -L "$staging" ] || [ -e "$staging/mbx-single-bundle" ]; then
    cache_unavailable "imported-bundle-retire-uncertain"
fi
if ! staging_real=$(CDPATH= cd -- "$staging" && pwd -P) || [ "$staging_real" != "$staging" ]; then
    cache_unavailable "imported-bundle-staging-path-mismatch"
fi
publish_state imported accepted restored-bundle-imported
exit 0"#;
