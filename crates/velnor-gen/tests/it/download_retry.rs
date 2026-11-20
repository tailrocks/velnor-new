// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::io::unique_id;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::release_symlink::Scratch;

type TestResult = Result<(), Box<dyn Error>>;

const CURL: &str = r#"#!/bin/sh
retry=no
all=no
previous=
for argument in "$@"; do
  [ "$previous" = "--retry" ] && [ "$argument" -ge 1 ] 2> /dev/null && retry=yes
  [ "$argument" = "--retry-all-errors" ] && all=yes
  previous=$argument
  url=$argument
done
if [ "$retry" != yes ] || [ "$all" != yes ]; then
  echo "curl: (22) The requested URL returned error: 503" >&2
  exit 22
fi
case "$url" in
  */SHA256SUMS) cat "$(dirname "$0")/SHA256SUMS" ;;
  */COMMIT) cat "$(dirname "$0")/COMMIT" ;;
  *) echo "curl: (22) The requested URL returned error: 404" >&2; exit 22 ;;
esac
"#;

#[test]
fn a_release_metadata_download_asks_curl_to_retry_so_a_transient_error_does_not_fail_the_pin()
-> TestResult {
    let base = Scratch(std::env::temp_dir().join(format!("velnor-gen-retry-{}", unique_id())));
    let bin = base.0.join("bin");
    let repo = base.0.join("repo");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&repo)?;
    fs::write(bin.join("SHA256SUMS"), SUMS)?;
    fs::write(bin.join("COMMIT"), COMMIT_FILE)?;
    let curl = bin.join("curl");
    fs::write(&curl, CURL)?;
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "pin-helper", "0.1.81"])
        .current_dir(&repo)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(repo.join(".velnor/rust-ci.lock").is_file());
    Ok(())
}
