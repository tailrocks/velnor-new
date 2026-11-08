// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::cell::Cell;
use std::process::Command;
use std::rc::Rc;
use std::time::Instant;
use velnor_actions_mise::{BoundedCommandTermination, run_command_bounded};

#[test]
fn timeout_retains_unread_child_output_and_preserves_foreign_process()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let marker = directory.path().join("written");
    let observed = Rc::new(Cell::new(false));
    let observed_by_clock = Rc::clone(&observed);
    let mut child = Command::new("/bin/sh");
    child
        .arg("-c")
        .arg("printf 'stdout-prefix'; printf 'stderr-prefix' >&2; : > \"$MARKER\"; exec sleep 120")
        .env("MARKER", &marker);
    let mut sentinel = Command::new("/bin/sleep").arg("120").spawn()?;
    let output = CheckDeadline::with_clock(
        move || {
            let until = Instant::now() + Duration::from_secs(10);
            while !marker.exists() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            observed_by_clock.set(marker.exists());
            Instant::now() + Duration::from_secs(3_601)
        },
        || {
            run_command_bounded(
                &mut child,
                6,
                Duration::from_secs(3_600),
                Duration::from_secs(2),
            )
        },
    );
    let alive = sentinel.try_wait()?.is_none();
    sentinel.kill()?;
    sentinel.wait()?;
    assert!(observed.get(), "child did not reach the output marker");
    assert!(alive, "unrelated process was terminated");
    assert_eq!(output.termination, BoundedCommandTermination::TimedOut);
    assert_eq!(output.cleanup_error, None);
    assert_eq!(output.stdout, b"stdout");
    assert_eq!(output.stderr, b"stderr");
    directory.close()?;
    Ok(())
}
