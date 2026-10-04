set -euo pipefail
umask 077
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
selected_root="${MBX_SELECTED_CACHE_ROOT:-${MBX_CACHE_DIR-}}"
abandoned_root=''
if [ -s "$evidence/original-cache-root.txt" ]; then
  IFS= read -r abandoned_root < "$evidence/original-cache-root.txt"
fi
if [ -s "$evidence/fallback-cache-root.txt" ]; then
  IFS= read -r selected_root < "$evidence/fallback-cache-root.txt"
fi
receipt_tmp="$evidence/cache-receipt.json.tmp"
test ! -e "$receipt_tmp"
jq -cn \
  --arg run_id "$GITHUB_RUN_ID" --arg run_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg sha "$GITHUB_SHA" --arg ref "$GITHUB_REF" --arg workflow_ref "$GITHUB_WORKFLOW_REF" \
  --arg runner_os "$RUNNER_OS" --arg runner_arch "$RUNNER_ARCH" \
  --arg image_os "${ImageOS-}" --arg image_version "${ImageVersion-}" \
  --arg mbx_version "${MBX_VERSION-}" --arg rust_toolchain "${RUSTUP_TOOLCHAIN-}" \
  --arg action_ref "$MBX_QUALIFICATION_ACTION_REF" --arg cache_scope "$MBX_CACHE_SCOPE" \
  --arg cache_primary "$MBX_QUALIFICATION_CACHE_PRIMARY" \
  --arg cache_prefix "$MBX_QUALIFICATION_CACHE_PREFIX" \
  --arg cache_matched_key "$MBX_QUALIFICATION_CACHE_MATCHED_KEY" \
  --arg cache_hit "$MBX_QUALIFICATION_CACHE_HIT" \
  --arg restore_primary_key "$MBX_QUALIFICATION_RESTORE_PRIMARY_KEY" \
  --arg restore_conclusion "$MBX_QUALIFICATION_RESTORE_CONCLUSION" \
  --arg export_ready "$MBX_QUALIFICATION_EXPORT_READY" \
  --arg export_status "$MBX_QUALIFICATION_EXPORT_STATUS" \
  --arg gc_status "$MBX_QUALIFICATION_GC_STATUS" \
  --arg save_outcome "$MBX_QUALIFICATION_SAVE_OUTCOME" \
  --arg job_id "$MBX_QUALIFICATION_JOB_ID" --arg role "$MBX_QUALIFICATION_ROLE" \
  --arg generation "$MBX_QUALIFICATION_CACHE_GENERATION" \
  --arg rustc_identity "$MBX_QUALIFICATION_RUSTC_IDENTITY" \
  --arg imported_objects "$(jq -r '.objects // empty' "$evidence/mbx-cache-stats-import-step-end.json" 2>/dev/null || true)" \
  --arg cached_compilations "$(jq -r '.savings.cached_compilations // empty' "$evidence/mbx-stats-build-end.json" 2>/dev/null || true)" \
  --arg import_status "$(tail -n 1 "$MBX_QUALIFICATION_IMPORT_RECEIPT" 2>/dev/null | sed -n 's/^exit_status=//p' || true)" \
  --arg import_receipt_path "$MBX_QUALIFICATION_IMPORT_RECEIPT" \
  --arg selected_root "$selected_root" --arg abandoned_root "$abandoned_root" \
  --arg bundle "$RUNNER_TEMP/mbx-single-bundle" --arg cargo_home "$CARGO_HOME" \
  '{receipt_status:"provisional",job_id:$job_id,role:$role,run_id:$run_id,run_attempt:$run_attempt,source_sha:$sha,source_ref:$ref,workflow_ref:$workflow_ref,runner_os:$runner_os,runner_arch:$runner_arch,image_os:$image_os,image_version:$image_version,mbx_action_ref:$action_ref,mbx_version:$mbx_version,rust_version:$rust_toolchain,generation:$generation,rustc_identity:$rustc_identity,scope:$cache_scope,primary_key:$cache_primary,derived_primary_key:$cache_primary,restore_primary_key:$restore_primary_key,restore_conclusion:$restore_conclusion,restore_miss_candidate:($cache_primary != "" and $restore_conclusion == "success" and $restore_primary_key == $cache_primary and $cache_hit == "" and $cache_matched_key == ""),cache_prefix:$cache_prefix,matched_key:$cache_matched_key,cache_hit:$cache_hit,export_ready:$export_ready,export_status:$export_status,gc_status:$gc_status,save_outcome:$save_outcome,imported_objects:(try ($imported_objects|tonumber) catch null),cached_compilations:(try ($cached_compilations|tonumber) catch null),import_status:$import_status,import_receipt_path:$import_receipt_path,selected_cache_root:$selected_root,selected_import_root:$selected_root,abandoned_import_root:$abandoned_root,bundle:$bundle,cargo_home:$cargo_home}' \
  > "$receipt_tmp"
mv -T -- "$receipt_tmp" "$evidence/cache-receipt.json"
