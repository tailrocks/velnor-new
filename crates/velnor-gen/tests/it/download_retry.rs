// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use velnor_rust::pin_lock;

use crate::io::unique_id;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::release_symlink::Scratch;

type TestResult = Result<(), Box<dyn Error>>;

const CURL: &str = r#"#!/bin/sh
for argument in "$@"; do
  url=$argument
done
case "$url" in
  */SHA256SUMS) name=SHA256SUMS ;;
  */COMMIT) name=COMMIT ;;
  *) echo "curl: (22) The requested URL returned error: 404" >&2; exit 22 ;;
esac
dir="$(dirname "$0")"
if [ ! -e "$dir/failed-$name" ]; then
  : > "$dir/failed-$name"
  head -c 16 "$dir/$name"
  echo "curl: (18) transfer closed with outstanding read data remaining" >&2
  exit 18
fi
cat "$dir/$name"
"#;

#[test]
fn a_release_metadata_download_that_fails_midway_is_retried_without_its_partial_bytes() -> TestResult
{
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
    assert_eq!(
        fs::read_to_string(repo.join(".velnor/rust-ci.lock"))?,
        pin_lock("0.1.81", COMMIT_FILE, SUMS, None)?
    );
    Ok(())
}
