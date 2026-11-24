// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::Duration;

use velnor_gen::io::{Error as IoError, retry_download};
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

#[test]
fn a_download_that_keeps_failing_is_tried_four_times_waiting_1_2_and_4_seconds_and_returns_the_last_error()
 {
    let mut calls = 0;
    let waits = RefCell::new(Vec::new());
    let result: Result<Vec<u8>, IoError> = retry_download(
        |wait| waits.borrow_mut().push(wait),
        || {
            calls += 1;
            Err(IoError::Repository(format!("attempt {calls} failed")))
        },
    );
    assert_eq!(calls, 4);
    assert_eq!(waits.into_inner(), [1, 2, 4].map(Duration::from_secs));
    assert!(
        matches!(&result, Err(IoError::Repository(message)) if message == "attempt 4 failed"),
        "{result:?}"
    );
}

#[test]
fn a_download_that_succeeds_on_the_third_attempt_waits_1_and_2_seconds_and_returns_its_bytes() {
    let mut calls = 0;
    let waits = RefCell::new(Vec::new());
    let result = retry_download(
        |wait| waits.borrow_mut().push(wait),
        || {
            calls += 1;
            if calls < 3 {
                Err(IoError::Repository(format!("attempt {calls} failed")))
            } else {
                Ok(b"release bytes".to_vec())
            }
        },
    );
    assert_eq!(calls, 3);
    assert_eq!(waits.into_inner(), [1, 2].map(Duration::from_secs));
    assert_eq!(result.ok(), Some(b"release bytes".to_vec()));
}
