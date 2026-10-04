REAL_JQ="$(command -v jq)"
export REAL_JQ
cat > "$WORK/bin/jq" <<'SH'
#!/usr/bin/bash
if "$REAL_JQ" "$@"; then status=0; else status=$?; fi
if [[ "$status" == 0 && "${RESOURCE_JSON_INJECT_MODE:-}" == summary &&
  -n "${RESOURCE_JSON_INJECT_MARKER:-}" && ! -e "$RESOURCE_JSON_INJECT_MARKER" ]]; then
  summary_source=0
  for argument in "$@"; do [[ "$argument" != "$EVIDENCE/samples.jsonl" ]] || summary_source=1; done
  if (( summary_source == 1 )); then
    temporary="$EVIDENCE/sampling-summary.json.injected"
    test ! -e "$temporary" && test ! -L "$temporary" || exit 1
    printf '{"resource_samples_complete":false,"sample_count":0,"maximum_sample_gap_seconds":0,"sample_interval_seconds":1}\n' > "$temporary"
    cat "$EVIDENCE/sampling-summary.json" >> "$temporary"
    /usr/bin/mv -T -- "$temporary" "$EVIDENCE/sampling-summary.json"
    : > "$RESOURCE_JSON_INJECT_MARKER"
  fi
fi
exit "$status"
SH
chmod 700 "$WORK/bin/jq"

verify_resource_json_parser() {
  local root="$WORK/json-evidence" file kind
  mkdir -m 700 "$root"
  for kind in stats receipt summary metadata; do
    file="$root/$kind.json"
    printf '{"ok":true}\n' > "$file"
    expect_success "$kind single object" resource_json_single_object "$root" "$file"
    printf '{"ok":false}\n{"ok":true}\n' > "$file"
    expect_failure "$kind invalid then valid documents" resource_json_single_object "$root" "$file"
    printf '[]\n' > "$file"
    expect_failure "$kind array" resource_json_single_object "$root" "$file"
    printf 'null\n' > "$file"
    expect_failure "$kind null" resource_json_single_object "$root" "$file"
    : > "$file"
    expect_failure "$kind empty" resource_json_single_object "$root" "$file"
    printf '{"broken":' > "$file"
    expect_failure "$kind malformed" resource_json_single_object "$root" "$file"
    head -c 65537 /dev/zero > "$file"
    expect_failure "$kind oversized malformed" resource_json_single_object "$root" "$file"
  done
}

run_resource_json_rejections() {
  local kind file saved
  for kind in stats receipt summary metadata; do
    setup_case "json-$kind"
    if [[ "$kind" == summary ]]; then
      RESOURCE_JSON_INJECT_MODE=summary
      RESOURCE_JSON_INJECT_MARKER="$CASE_ROOT/summary-injection.marker"
      export RESOURCE_JSON_INJECT_MODE RESOURCE_JSON_INJECT_MARKER
    fi
    unset SPAWN_CHILD_PID SPAWN_CHILD_FILE
    set_hit_role reader
    MBX_QUALIFICATION_EXPECTED_INVENTORIES='restore-step-end import-step-end build-end'
    export MBX_QUALIFICATION_EXPECTED_INVENTORIES
    expect_start_success "json $kind finalizer start" || return 1
    populate_bundle
    snapshot restore-step-end
    snapshot import-step-end
    snapshot build-end
    for _ in {1..40}; do
      samples=$(wc -l < "$EVIDENCE/samples.jsonl")
      (( samples >= 2 )) && break
      /usr/bin/sleep 0.2
    done
    write_valid_receipts
    case "$kind" in
      stats) file="$EVIDENCE/mbx-cache-stats-import-step-end.json" ;;
      receipt) file="$EVIDENCE/cache-receipt.json" ;;
      metadata) file="$EVIDENCE/runner-metadata.json" ;;
    esac
    if [[ "$kind" == summary ]]; then
      :
    else
      saved="$CASE_ROOT/$kind.valid.json"
      cp "$file" "$saved"
      printf '{"invalid":true}\n' > "$file"
      cat "$saved" >> "$file"
    fi
    stop_and_check_partial || fail "finalizer completed with multiple $kind documents"
    case "$kind" in
      summary) grep -q 'resource_sampling_incomplete' "$EVIDENCE/qualification-errors.txt" || fail 'summary stream rejection missing' ;;
      *) grep -q "invalid_json:$file" "$EVIDENCE/qualification-errors.txt" || fail "$kind stream rejection missing" ;;
    esac
    if [[ "$kind" == summary ]]; then
      [[ -e "$RESOURCE_JSON_INJECT_MARKER" ]] || fail 'summary stream was not injected after generation'
      unset RESOURCE_JSON_INJECT_MODE RESOURCE_JSON_INJECT_MARKER
    fi
  done
}

run_complete_observer_finalizers() {
  local role
  for role in observer-shared observer-new; do
    setup_case "complete-$role"
    unset SPAWN_CHILD_PID SPAWN_CHILD_FILE
    set_hit_role "$role"
    MBX_QUALIFICATION_EXPECTED_INVENTORIES='restore-step-end import-step-end build-end'
    export MBX_QUALIFICATION_EXPECTED_INVENTORIES
    expect_start_success "$role evidence start" || return 1
    populate_bundle
    snapshot restore-step-end
    snapshot import-step-end
    snapshot build-end
    for _ in {1..40}; do
      samples=$(wc -l < "$EVIDENCE/samples.jsonl")
      (( samples >= 2 )) && break
      /usr/bin/sleep 0.2
    done
    write_valid_receipts
    if stop_sampler >/dev/null 2>&1 && grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv"; then
      pass "$role retains complete cache and resource proof"
      assert_provisional_certification
    else
      fail "$role finalizer did not retain complete proof"
      cat "$EVIDENCE/qualification-errors.txt" >&2 2>/dev/null || true
    fi
  done
}
