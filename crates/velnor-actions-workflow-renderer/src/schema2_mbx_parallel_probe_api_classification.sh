write_restore_classification() {
  local output="$1" temporary metadata bytes value links mode owner group
  shift
  verify_directory "$evidence" "$runner_temp_real/mbx-cache-evidence" 700 || return 1
  case "$output" in
    "$seed_restore_classification_file"|"$reader_a_restore_classification_file"|"$reader_b_restore_classification_file"|"$new_writer_restore_classification_file"|"$observer_restore_classification_file") ;;
    *) return 1 ;;
  esac
  temporary="$output.tmp"
  [[ ! -e "$temporary" && ! -L "$temporary" ]] || return 1
  if (($# == 0)); then printf 'NOT_RUN\n' > "$temporary"; else "$@" > "$temporary"; fi
  metadata="$(stat -c '%h:%a:%u:%g:%s' -- "$temporary" 2>/dev/null)" || return 1
  IFS=: read -r links mode owner group bytes <<< "$metadata"
  [[ "$links:$mode:$owner:$group" == "1:600:$(id -u):$(id -g)" && "$bytes" =~ ^[0-9]{1,3}$ ]] || return 1
  (( 10#$bytes <= 32 )) || return 1
  value="$(cat -- "$temporary")"
  case "$value" in NOT_RUN|HIT|CLEAN_MISS) ;; *) return 1 ;; esac
  if [[ -e "$output" || -L "$output" ]]; then
    [[ -f "$output" && ! -L "$output" && -O "$output" &&
      "$(stat -c '%h:%a:%u:%g' -- "$output" 2>/dev/null)" == "1:600:$(id -u):$(id -g)" ]] || return 1
  fi
  mv -T -- "$temporary" "$output"
}
