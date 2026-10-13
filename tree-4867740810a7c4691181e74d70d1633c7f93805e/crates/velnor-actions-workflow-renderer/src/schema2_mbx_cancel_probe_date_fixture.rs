//! Fixed timestamp conversion responses for upload-window fixtures.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(super) fn install(bin: &Path) -> io::Result<()> {
    let date = bin.join("date");
    fs::write(
        &date,
        r#"#!/usr/bin/env bash
set -euo pipefail
if [ "$#" = 4 ] && [ "$1" = -u ] && [ "$2" = -d ] && [ "$4" = +%s%N ]; then
  case "$3" in
    2026-10-04T00:00:05.0000000Z) printf '5\n' ;;
    2026-10-04T00:00:10Z) printf '10\n' ;;
    2026-10-04T00:00:11.0000000Z) printf '11\n' ;;
    *) exit 1 ;;
  esac
elif [ "$#" = 4 ] && [ "$1" = -u ] && [ "$2" = -d ] \
  && [ "$4" = '+%Y-%m-%dT%H:%M:%S.%7NZ' ]; then
  case "$3" in
    2026-10-04T00:00:05.0000000Z) printf '%s\n' "$3" ;;
    2026-10-04T00:00:11.0000000Z) printf '%s\n' "$3" ;;
    *) exit 1 ;;
  esac
else
  exec /bin/date "$@"
fi
"#,
    )?;
    fs::set_permissions(date, fs::Permissions::from_mode(0o755))
}
