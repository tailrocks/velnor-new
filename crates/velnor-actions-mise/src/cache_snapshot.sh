# Fixed generator-owned opaque observation. No archived program is executed.
# Bookkeeping failures suppress exports and preserve the producer outcome.
set -uf
layer="${1-}"
phase="${2-}"
export LC_ALL=C

unavailable() {
  printf 'velnor: %s snapshot unavailable; export suppressed\n' "$layer" >&2
  if [ -n "${output-}" ]; then
    printf 'VELNOR_%s_SNAPSHOT_CHANGED=false\nVELNOR_%s_SNAPSHOT_DIGEST=\n' \
      "$output" "$output" >> "${GITHUB_ENV-}" || return 0
  fi
  printf 'changed=false\navailable=false\n' >> "${GITHUB_OUTPUT-}" || return 0
}

output_unavailable=false
printf 'available=false\n' >> "${GITHUB_OUTPUT-}" || output_unavailable=true
case "$layer" in
@DOMAINS@
  *) unavailable; exit 0 ;;
esac
if [ "$output_unavailable" = true ]; then
  unavailable
  exit 0
fi

prepare() {
  case "$phase" in before|after) ;; *) return 1 ;; esac
  [ "${VELNOR_SNAPSHOT_PHASE-}" = "$phase" ] || return 1
  [ "${VELNOR_SNAPSHOT_LAYER-}" = "$layer" ] || return 1
  [ "${VELNOR_SNAPSHOT_OUTPUT-}" = "$output" ] || return 1
  [ "${VELNOR_SNAPSHOT_ROOTS-}" = "$roots" ] || return 1
  [ -n "${RUNNER_TEMP-}" ] || return 1
  owned_root="$RUNNER_TEMP/velnor"
  snapshot_dir="$owned_root/cache-snapshots"
  before="$snapshot_dir/$layer-before"
  [ ! -L "$owned_root" ] || return 1
  [ ! -e "$owned_root" ] || [ -d "$owned_root" ] || return 1
  mkdir -p "$owned_root" || return 1
  [ ! -L "$snapshot_dir" ] || return 1
  [ ! -e "$snapshot_dir" ] || [ -d "$snapshot_dir" ] || return 1
  mkdir -p "$snapshot_dir" || return 1
  [ ! -L "$before" ] || return 1
  [ ! -e "$before" ] || [ -f "$before" ] || return 1
}

read_summary() {
  IFS=' ' read -r digest files bytes extra <<VELNOR_INVENTORY_SUMMARY
$summary
VELNOR_INVENTORY_SUMMARY
  case "$digest" in *[!0-9a-f]*|'') return 1 ;; esac
  [ "${#digest}" -eq 64 ] || return 1
  case "$files" in *[!0-9]*|'') return 1 ;; esac
  case "$bytes" in *[!0-9]*|'') return 1 ;; esac
  [ -z "$extra" ]
}

observe() {
  read_summary || return 1
  elapsed=$(( $(date +%s) - started ))
  changed=false
  if [ "$phase" = before ]; then
    printf '%s\n' "$digest" > "$before" || return 1
  else
    [ -f "$before" ] || return 1
    previous=$(head -c 65 "$before") || return 1
    case "$previous" in *[!0-9a-f]*|'') return 1 ;; esac
    [ "${#previous}" -eq 64 ] || return 1
    if [ "$files" -gt 0 ] && { [ "$digest" != "$previous" ] || [ -z "${VELNOR_SNAPSHOT_RESTORED-}" ]; }; then changed=true; fi
    printf 'VELNOR_%s_SNAPSHOT_DIGEST=%s\nVELNOR_%s_SNAPSHOT_CHANGED=%s\n' \
      "$output" "$digest" "$output" "$changed" >> "${GITHUB_ENV-}" || return 1
    printf 'changed=%s\n' "$changed" >> "${GITHUB_OUTPUT-}" || return 1
  fi
  printf 'digest=%s\n' "$digest" >> "${GITHUB_OUTPUT-}" || return 1
  printf 'velnor: %s snapshot phase=%s files=%s bytes=%s hash_seconds=%s hash_resolution_seconds=1 digest=%s\n' \
    "$layer" "$phase" "$files" "$bytes" "$elapsed" "$digest" || return 1
  printf 'available=true\n' >> "${GITHUB_OUTPUT-}" || return 1
}

if ! prepare || ! started=$(date +%s); then
  unavailable
  exit 0
fi
if ! summary=$(/usr/bin/python3 -I -S - "$owned_root" "$roots_json" <<'VELNOR_SHARED_INVENTORY'
@ENGINE@
@ENTRY@
VELNOR_SHARED_INVENTORY
); then
  unavailable
  exit 0
fi
if ! observe; then
  unavailable
fi
exit 0
