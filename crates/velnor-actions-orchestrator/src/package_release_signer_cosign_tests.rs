// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

use super::{COSIGN_VERSION, acquire_cosign_with};

#[test]
fn cosign_download_obeys_remaining_operation_deadline() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let started = Instant::now();
    let deadline = CheckDeadline::after(Duration::from_millis(1500))?;
    let marker = temp.path().join("download-started");
    std::thread::sleep(Duration::from_millis(600));
    let result = acquire_cosign_with(temp.path(), deadline, || {
        IsolatedCommand::repo_task(
            "/bin/sh",
            vec![
                OsString::from("-c"),
                OsString::from("touch \"$1\"; sleep 3"),
                OsString::from("fixture"),
                marker.as_os_str().to_owned(),
            ],
            &[],
        )
    });
    assert!(result.is_err(), "stalled download was admitted");
    assert!(marker.is_file(), "download process did not start");
    assert!(
        started.elapsed() < Duration::from_millis(2600),
        "download exceeded remaining deadline"
    );
    let tools = temp.path().join("velnor/tools");
    assert!(!tools.join(format!("cosign-{COSIGN_VERSION}")).exists());
    assert!(
        !tools
            .join(format!("cosign-{COSIGN_VERSION}.download"))
            .exists()
    );
    Ok(())
}
