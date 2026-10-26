// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const SIDECAR_AWK: &str = "NR == 1 { if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1; print $1; next } { exit 1 } END { if (NR != 1) exit 1 }";

pub(super) fn sidecar_digest_command(sidecar: &str, binary: &str) -> String {
    format!("awk -v expected='{binary}' '{SIDECAR_AWK}' '{sidecar}'")
}

pub(super) fn raw_artifact_validation_script(
    directory: &str,
    binary: &str,
    sidecar: &str,
    provenance: &str,
) -> String {
    format!(
        r#"set -euo pipefail
dir='{directory}'
test -d "$dir"
test ! -L "$dir"
shopt -s dotglob nullglob
entries=("$dir"/*)
test "${{#entries[@]}}" -eq 3
total=0
for path in "${{entries[@]}}"; do
  case "$path" in
    "$dir/{binary}"|"$dir/{sidecar}"|"$dir/{provenance}") ;;
    *) echo 'unexpected release artifact entry' >&2; exit 1 ;;
  esac
  test -f "$path"
  test ! -L "$path"
  test -s "$path"
  test ! -x "$path"
  links="$(find "$path" -type f -links 1 -print -quit)"
  test "$links" = "$path"
  size="$(wc -c < "$path")"
  size="${{size//[[:space:]]/}}"
  [[ "$size" =~ ^[0-9]+$ ]] || exit 1
  test "$size" -le 268435456
  total=$((total + size))
  test "$total" -le 268435456
done"#
    )
}
