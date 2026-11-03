// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

pub(crate) const EXPORT_MARK_ENV: &str = "MBX_STUB_EXPORT_MARK";
pub(crate) const EXPORT_SPARSE_BYTES_ENV: &str = "MBX_STUB_EXPORT_SPARSE_BYTES";
pub(crate) const EXPORT_FILES_ENV: &str = "MBX_STUB_EXPORT_FILES";
pub(crate) const ZSTD_PAD_ENV: &str = "ZSTD_SHIM_PAD_TO";
pub(crate) const UNAME_S_ENV: &str = "VELNOR_STUB_UNAME_S";
pub(crate) const UNAME_M_ENV: &str = "VELNOR_STUB_UNAME_M";
pub(crate) const FAIL_ENV: &str = "MBX_STUB_FAIL";

const MBX: &str = r#"#!/bin/sh
export LC_ALL=C
[ "$1 $2" = "cache import" ] || [ "$1 $2" = "cache export" ] || exit 1
op="$2"
[ "${MBX_STUB_FAIL:-}" = "$op" ] && exit 1
if [ "$op" = export ]; then
  [ "$#" -eq 7 ] && [ "$3" = --group ] && [ -n "$4" ] && [ "$5" = --format ] && [ "$6" = directory ] || exit 1
  dir="$7"
  mkdir -p "$dir/cas" "$dir/action-results" || exit 1
  printf '%s' "${MBX_STUB_EXPORT_MARK:-fresh}" > "$dir/cas/object" || exit 1
  printf '%s' "$4" > "$dir/action-results/result" || exit 1
  printf '{"version":1}\n' > "$dir/mbx-cache-export-v1.json" || exit 1
  sparse="${MBX_STUB_EXPORT_SPARSE_BYTES:-0}"
  if [ "$sparse" -gt 0 ]; then dd if=/dev/null of="$dir/cas/sparse" bs=1 seek="$sparse" count=0 2>/dev/null || exit 1; fi
  files="${MBX_STUB_EXPORT_FILES:-0}"
  i=0
  while [ "$i" -lt "$files" ]; do : > "$dir/cas/f$i" || exit 1; i=$((i + 1)); done
  find "$dir" -exec touch -t 200001010000 {} + || exit 1
  exit 0
fi
[ "$#" -eq 3 ] || exit 1
dir="$3"
[ -d "$dir" ] && [ ! -L "$dir" ] || exit 1
[ "$(ls -A "$dir")" = "$(printf 'action-results\ncas\nmbx-cache-export-v1.json')" ] || exit 1
[ -d "$dir/cas" ] && [ ! -L "$dir/cas" ] || exit 1
[ -d "$dir/action-results" ] && [ ! -L "$dir/action-results" ] || exit 1
[ -f "$dir/mbx-cache-export-v1.json" ] && [ ! -L "$dir/mbx-cache-export-v1.json" ] || exit 1
[ -z "$(find "$dir" ! -type f ! -type d)" ] || exit 1
[ -z "$(find "$dir" -type f -links +1)" ] || exit 1
[ -n "${MBX_CACHE_DIR:-}" ] || exit 1
mkdir -p "$MBX_CACHE_DIR/cas" "$MBX_CACHE_DIR/action-results" || exit 1
for part in cas action-results; do
  if [ -e "$MBX_CACHE_DIR/$part" ]; then cp -R "$dir/$part/." "$MBX_CACHE_DIR/$part/" || exit 1; else mv "$dir/$part" "$MBX_CACHE_DIR/$part" || exit 1; fi
done
rm -rf "$dir"
here="$PWD"
while [ -n "$here" ]; do
  if [ -f "$here/Cargo.lock" ]; then
    if [ ! -e "$here/target" ] || [ -z "$(ls -A "$here/target")" ]; then mkdir -p "$here/target/debug"; fi
    break
  fi
  here="${here%/*}"
done
exit 0
"#;

const UNAME: &str = "#!/bin/sh\ncase \"$1\" in\n  -s) [ -z \"${VELNOR_STUB_UNAME_S:-}\" ] || { printf '%s\\n' \"$VELNOR_STUB_UNAME_S\"; exit 0; } ;;\n  -m) [ -z \"${VELNOR_STUB_UNAME_M:-}\" ] || { printf '%s\\n' \"$VELNOR_STUB_UNAME_M\"; exit 0; } ;;\nesac\nexec /usr/bin/uname \"$@\"\n";

const MISE: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --) shift; break ;; *) shift ;; esac; done\nexec \"$@\"\n";

const ZSTD_PAD: &str = r#"#!/bin/sh
real="$ZSTD_SHIM_REAL"
for a in "$@"; do case "$a" in -d|-dc|-cd) exec "$real" "$@" ;; esac; done
"$real" "$@" || exit $?
[ -n "${ZSTD_SHIM_PAD_TO:-}" ] || exit 0
out=; prev=; for a in "$@"; do [ "$prev" = -o ] && out=$a; prev=$a; done
len=$(wc -c < "$out")
need=$((ZSTD_SHIM_PAD_TO - len))
[ "$need" -ne 0 ] || exit 0
data=$((need - 8))
printf '\120\052\115\030' >> "$out"
printf "$(printf '\\%03o\\%03o\\%03o\\%03o' $((data & 255)) $(((data >> 8) & 255)) $(((data >> 16) & 255)) $(((data >> 24) & 255)))" >> "$out"
head -c "$data" /dev/zero >> "$out"
"#;

pub(crate) fn install_zstd_pad(bin: &Path) -> Result<(), Box<dyn Error>> {
    let path = std::env::var("PATH")?;
    let real = std::env::split_paths(&path)
        .map(|dir| dir.join("zstd"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| std::io::Error::other("no zstd on PATH"))?;
    write_executable(
        &bin.join("zstd"),
        &ZSTD_PAD.replacen("$ZSTD_SHIM_REAL", &real.display().to_string(), 1),
    )
}

pub(crate) fn write_executable(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, body)?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

pub(crate) fn install(bin: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(bin)?;
    write_executable(&bin.join("mbx"), MBX)?;
    write_executable(&bin.join("uname"), UNAME)?;
    write_executable(&bin.join("mise"), MISE)
}

pub(crate) fn install_failure(bin: &Path, tool: &str) -> Result<(), Box<dyn Error>> {
    write_executable(&bin.join(tool), "#!/bin/sh\nexit 1\n")
}
