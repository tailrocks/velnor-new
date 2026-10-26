// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const BIND_SELECTED_MISE_FUNCTION: &str = r#"velnor_capture_physical_path() {
  [ "$#" -eq 2 ] || return 1
  (cd -P "$1" && pwd -P) > "$2" || return 1
  resolved_path=
  newline='
'
  while IFS= read -r path_line; do
    resolved_path=$resolved_path$path_line"$newline"
  done < "$2"
  case "$resolved_path" in
    *"$newline") resolved_path=${resolved_path%"$newline"} ;;
    *) return 1 ;;
  esac
  printf '%s\n' "$resolved_path" > "$2" || return 1
}; velnor_bind_selected_mise() { [ "$#" -eq 1 ] || return 5; mise_data_dir=${MISE_DATA_DIR-}; if [ -z "$mise_data_dir" ]; then if [ -n "${XDG_DATA_HOME-}" ]; then mise_data_dir="$XDG_DATA_HOME/mise"; elif [ -n "${HOME-}" ]; then mise_data_dir="$HOME/.local/share/mise"; else return 1; fi; fi; case "$mise_data_dir" in /*) ;; *) mise_data_dir=./$mise_data_dir;; esac; case "$mise_data_dir" in /) ;; */) mise_data_dir=${mise_data_dir%/};; esac; [ -d "$mise_data_dir" ] || return 2; velnor_capture_physical_path "$mise_data_dir" "$1" || return 3; resolved_mise_data_dir=$resolved_path; [ -d "$resolved_mise_data_dir/bin" ] || return 4; velnor_capture_physical_path "$resolved_mise_data_dir/bin" "$1" || return 4; resolved_mise_bin_dir=$resolved_path; mise_bin="$resolved_mise_bin_dir/mise"; [ -f "$mise_bin" ] && [ -x "$mise_bin" ] && [ ! -L "$mise_bin" ] || return 4; MISE_DATA_DIR="$resolved_mise_data_dir"; export MISE_DATA_DIR; return 0; }"#;
