set_cold_role() {
  export MBX_QUALIFICATION_ROLE="$1"
  export MBX_QUALIFICATION_CACHE_HIT='' MBX_QUALIFICATION_CACHE_MATCHED_KEY=''
  export MBX_QUALIFICATION_RESTORE_PRIMARY_KEY="$MBX_QUALIFICATION_CACHE_PRIMARY"
  export MBX_QUALIFICATION_RESTORE_CONCLUSION=success MBX_QUALIFICATION_EXPORT_READY=true
}

set_hit_role() {
  export MBX_QUALIFICATION_ROLE="$1"
  export MBX_QUALIFICATION_CACHE_HIT=true MBX_QUALIFICATION_CACHE_MATCHED_KEY=primary
  export MBX_QUALIFICATION_RESTORE_PRIMARY_KEY=primary MBX_QUALIFICATION_RESTORE_CONCLUSION=success
}

cold_absent_bundle_boundary() {
  local role_name=$1
  rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
  if snapshot restore-step-end; then pass "$role_name allows absent restore bundle"; else fail "$role_name rejected absent restore bundle"; fi
  assert_bundle_row restore-step-end missing false || fail "$role_name absent bundle was not optional"
}

reader_absent_bundle_boundary() {
  rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
  if snapshot restore-step-end; then fail 'reader accepted absent restore bundle'; else pass 'reader rejects absent restore bundle at boundary'; fi
  assert_bundle_row restore-step-end missing true || fail 'reader absence was not recorded as required'
  populate_bundle
}

reader_empty_bundle_boundary() {
  rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
  mkdir -m 700 "$RUNNER_TEMP/mbx-single-bundle"
  if snapshot restore-step-end; then pass 'reader records empty bundle boundary'; else fail 'reader rejected existing empty bundle before finalizer'; fi
  assert_bundle_row restore-step-end present true || fail 'reader empty bundle was not marked required'
  populate_bundle
}

reader_nonempty_bundle_boundary() {
  if snapshot restore-step-end; then pass 'reader accepts nonempty restore bundle'; else fail 'reader rejected nonempty restore bundle'; fi
  assert_bundle_row restore-step-end present true || fail 'reader hit bundle was not required'
}

populate_bundle() {
  mkdir -p -m 700 "$RUNNER_TEMP/mbx-single-bundle/cache/cas/v1/blake3/aa"
  printf payload-original > "$RUNNER_TEMP/mbx-single-bundle/cache/cas/v1/blake3/aa/payload-01"
  printf bundle-unrelated > "$RUNNER_TEMP/mbx-single-bundle/unrelated.txt"
}

assert_bundle_row() {
  local label=$1 status=$2 required=$3
  awk -F '\t' -v status="$status" -v required="$required" \
    '$1 == "bundle" && $2 == status && $3 == required { found=1 } END { exit !found }' \
    "$EVIDENCE/root-status-$label.tsv"
}

assert_candidate() {
  local name=$1 expected=$2
  if jq -e --argjson expected "$expected" \
    '.receipt_status == "provisional" and .restore_miss_candidate == $expected' \
    "$EVIDENCE/cache-receipt.json" >/dev/null; then
    pass "$name receipt candidate=$expected remains provisional"
  else
    fail "$name receipt candidate/status mismatch"
  fi
}

assert_genuine_miss_receipt() {
  if jq -e '.receipt_status == "provisional" and .restore_miss_candidate == true and
      .primary_key == "primary" and .derived_primary_key == .primary_key and
      .restore_primary_key == .primary_key and .restore_conclusion == "success" and
      .cache_hit == "" and .matched_key == ""' \
      "$EVIDENCE/cache-receipt.json" >/dev/null; then
    pass 'stock blank-hit successful primary match yields provisional miss candidate'
  else
    fail 'stock miss receipt fields did not produce the expected provisional candidate'
  fi
}

reset_genuine_miss() {
  export MBX_QUALIFICATION_CACHE_PRIMARY=primary MBX_QUALIFICATION_RESTORE_PRIMARY_KEY=primary
  export MBX_QUALIFICATION_RESTORE_CONCLUSION=success MBX_QUALIFICATION_CACHE_HIT=''
  export MBX_QUALIFICATION_CACHE_MATCHED_KEY='' MBX_QUALIFICATION_EXPORT_READY=true
  export MBX_QUALIFICATION_EXPORT_STATUS=0 MBX_QUALIFICATION_GC_STATUS=0
  export MBX_QUALIFICATION_SAVE_OUTCOME=success
}

write_candidate_receipt() {
  if bash "$WORK/receipt.sh"; then return 0; fi
  fail 'fixed receipt script rejected a complete fixture environment'
  return 1
}

run_miss_candidate_matrix() {
  assert_genuine_miss_receipt

  export MBX_QUALIFICATION_RESTORE_CONCLUSION=''
  write_candidate_receipt && assert_candidate 'unavailable restore conclusion' false

  reset_genuine_miss
  export MBX_QUALIFICATION_RESTORE_PRIMARY_KEY=''
  write_candidate_receipt && assert_candidate 'unavailable restore primary key' false

  reset_genuine_miss
  export MBX_QUALIFICATION_CACHE_HIT=false
  write_candidate_receipt && assert_candidate 'explicit false cache-hit output' false

  reset_genuine_miss
  export MBX_QUALIFICATION_CACHE_PRIMARY='' MBX_QUALIFICATION_RESTORE_PRIMARY_KEY=''
  write_candidate_receipt && assert_candidate 'missing primary key' false

  reset_genuine_miss
  export MBX_QUALIFICATION_RESTORE_CONCLUSION=failure
  write_candidate_receipt && assert_candidate 'failed restore conclusion' false

  reset_genuine_miss
  printf 'Failed to Restore: fixture-only text, not a service trace\n' > "$EVIDENCE/restore-action.log"
  write_candidate_receipt && assert_genuine_miss_receipt
  printf '%s\n' 'NOT_RUN actual restore-error classification: terminal log classifier is absent'

  reset_genuine_miss
  write_candidate_receipt && assert_genuine_miss_receipt
}

assert_provisional_certification() {
  local lines
  grep -qx $'cache_certification\tprovisional' "$EVIDENCE/qualification-status.tsv" || \
    fail 'finalizer did not leave cache certification provisional'
  lines=$(wc -l < "$EVIDENCE/qualification-status.tsv")
  [[ "$lines" == 2 ]] || fail 'qualification status contains an unexpected terminal decision'
  if grep -Eiq 'qualified|certified|cache_miss' "$EVIDENCE/qualification-status.tsv"; then
    fail 'provisional evidence contains terminal cache certification'
  fi
}

verify_resource_role_classes() {
  local role expected actual
  source "$WORK/path-validation.sh"
  for role in writer seed new-key-writer; do
    export MBX_QUALIFICATION_ROLE="$role"
    if actual=$(resource_role_class) && [[ "$actual" == cold ]]; then
      pass "role classifier maps $role to cold"
    else
      fail "role classifier did not map $role to cold"
    fi
  done
  for role in reader reader-a reader-b corrupt-reader; do
    export MBX_QUALIFICATION_ROLE="$role"
    if actual=$(resource_role_class) && [[ "$actual" == hit ]]; then
      pass "role classifier maps $role to hit"
    else
      fail "role classifier did not map $role to hit"
    fi
  done
  export MBX_QUALIFICATION_ROLE=unknown-role
  if actual=$(resource_role_class); then
    fail 'role classifier accepted unknown role'
  else
    pass 'role classifier rejects unknown role'
  fi
}
