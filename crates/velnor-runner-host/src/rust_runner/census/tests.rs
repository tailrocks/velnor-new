// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{
    Held, marker_absent, wait_for_rust_runner_connection_drain, wait_for_rust_runner_drain, write,
};
use crate::config::ConnectionId;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const WAIT: Duration = Duration::from_millis(300);

#[test]
fn a_marker_is_absent_only_when_the_path_does_not_exist() -> Result<(), std::io::Error> {
    let directory = tempfile::tempdir()?;
    let present = directory.path().join("present");
    std::fs::write(&present, "")?;
    assert!(marker_absent(&directory.path().join("missing")));
    assert!(!marker_absent(&present));
    assert!(!marker_absent(&present.join("below a file")));
    Ok(())
}

fn drained(reconciled: bool) -> Result<(bool, bool), Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    std::fs::create_dir(state.path().join("connection-drain"))?;
    std::fs::write(state.path().join("connection-drain/lab"), TOKEN)?;
    std::fs::write(state.path().join("drain"), TOKEN)?;
    let held = vec![Held {
        id: "lab".to_owned(),
        slots: 0,
        admitting: false,
    }];
    write(state.path(), "hash", held, reconciled)?;
    Ok((
        wait_for_rust_runner_drain(state.path(), WAIT)?,
        wait_for_rust_runner_connection_drain(state.path(), &ConnectionId::parse("lab")?, WAIT)?,
    ))
}

#[test]
fn a_census_proves_a_drain_only_after_the_controller_reconciled()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(drained(true)?, (true, true));
    assert_eq!(drained(false)?, (false, false));
    Ok(())
}
