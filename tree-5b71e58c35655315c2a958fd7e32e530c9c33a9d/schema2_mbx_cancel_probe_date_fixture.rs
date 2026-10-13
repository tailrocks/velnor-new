//! Fixed calendar validation responses for hosted-log fixtures.

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
if [ "$#" = 4 ] && [ "$1" = -u ] && [ "$2" = -d ] \
  && [ "$4" = '+%Y-%m-%dT%H:%M:%S.%7NZ' ]; then
  case "$3" in
    2026-10-04T00:00:05.0000000Z) printf '%s\n' "$3" ;;
    2026-10-04T00:00:06.0000000Z) printf '%s\n' "$3" ;;
    2026-10-04T00:00:11.0000000Z) printf '%s\n' "$3" ;;
    2099-12-31T23:59:59.0000000Z) printf '%s\n' "$3" ;;
    1999-01-01T00:00:00.0000000Z) printf '%s\n' "$3" ;;
    *) exit 1 ;;
  esac
else
  exec /bin/date "$@"
fi
"#,
    )?;
    fs::set_permissions(date, fs::Permissions::from_mode(0o755))
}
