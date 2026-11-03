// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

pub(crate) const EXPORT_MARK_ENV: &str = "MBX_STUB_EXPORT_MARK";
pub(crate) const EXPORT_BULK_MIB_ENV: &str = "MBX_STUB_EXPORT_BULK_MIB";
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
  bulk="${MBX_STUB_EXPORT_BULK_MIB:-0}"
  if [ "$bulk" -gt 0 ]; then dd if=/dev/zero of="$dir/cas/bulk" bs=1048576 count="$bulk" 2>/dev/null || exit 1; fi
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

const MISE: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --) shift; break ;; *) shift ;; esac; done\nexec \"$@\"\n";

fn write_executable(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, body)?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

pub(crate) fn install(bin: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(bin)?;
    write_executable(&bin.join("mbx"), MBX)?;
    write_executable(&bin.join("mise"), MISE)
}
