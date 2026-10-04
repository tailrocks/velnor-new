set -euo pipefail
umask 077
root="$RUNNER_TEMP/mbx-roundtrip-terminal"
stock="$RUNNER_TEMP/mbx-stock-restore-evidence"
test -d "$root" && test ! -L "$root"
test -d "$stock" && test ! -L "$stock"
test "$(realpath -e -- "$root")" = "$root"
test "$(realpath -e -- "$stock")" = "$stock"
for directory in "$root" "$root/writer" "$root/reader" "$root/corrupt-reader" "$stock"; do
  stock_restore_private_dir "$directory" "$RUNNER_TEMP"
done

classify_receipt() {
  local file="$1" job="$2" role="$3" name="$4" evidence="$5"
  if ! stock_restore_private_file "$file" "$RUNNER_TEMP" 65536 \
    || ! jq -e --arg action "$MBX_EXPECTED_ACTION_REF" --arg version "$MBX_EXPECTED_VERSION" \
    --arg rust "$MBX_EXPECTED_RUST_VERSION" \
    '.mbx_action_ref == $action and .mbx_version == $version and .rust_version == $rust' \
    "$file" >/dev/null 2>&1; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  stock_restore_classify_receipt "$file" "$job" "$role" "$name" completed \
    "$MBX_EXPECTED_MODE" "$evidence"
}

writer="$(classify_receipt "$root/writer/cache-receipt.json" \
  mbx-cache-write-hosted writer 'MBX objects cache / protected-main writer' \
  "$stock/writer.json")"
reader="$(classify_receipt "$root/reader/cache-receipt.json" \
  mbx-cache-read-hosted reader 'MBX objects cache / read-only reuse' \
  "$stock/reader.json")"
corrupt_reader="$(classify_receipt "$root/corrupt-reader/cache-receipt.json" \
  mbx-cache-corrupt-import-hosted corrupt-reader 'MBX objects cache / corrupt import cold fallback' \
  "$stock/corrupt-reader.json")"

classification=QUALIFIED
if [ "$writer" != CLEAN_MISS ] || [ "$reader" != HIT ] || [ "$corrupt_reader" != HIT ]; then
  classification=NOT_RUN
fi
report="$stock/qualification.json"
test ! -e "$report" && test ! -L "$report"
temporary="$(mktemp "$stock/.qualification.XXXXXXXXXX")"
chmod 600 "$temporary"
jq -n --arg classification "$classification" --arg writer "$writer" \
  --arg reader "$reader" --arg corrupt_reader "$corrupt_reader" \
  --arg run_id "$GITHUB_RUN_ID" --arg run_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg source_sha "$GITHUB_SHA" --arg source_ref "$GITHUB_REF" \
  --arg workflow_ref "$GITHUB_WORKFLOW_REF" \
  '{schema_version:1,classification:$classification,run_id:$run_id,
    run_attempt:$run_attempt,source_sha:$source_sha,source_ref:$source_ref,
    workflow_ref:$workflow_ref,restore_steps:{
      writer:{job_id:"mbx-cache-write-hosted",role:"writer",classification:$writer},
      reader:{job_id:"mbx-cache-read-hosted",role:"reader",classification:$reader},
      corrupt_reader:{job_id:"mbx-cache-corrupt-import-hosted",role:"corrupt-reader",classification:$corrupt_reader}}}' \
  >| "$temporary"
ln -- "$temporary" "$report"
rm -f -- "$temporary"
printf 'MBX roundtrip terminal classification: %s\n' "$classification"
test "$classification" = QUALIFIED
