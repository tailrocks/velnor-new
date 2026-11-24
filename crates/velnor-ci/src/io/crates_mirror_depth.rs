// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Depth of a forwarded Cargo wrapper chain, read from the process tree.
//!
//! `VELNOR_CARGO_DEPTH` is only as reliable as the processes that pass it on: a
//! `cargo` shim can `unset` it and the count restarts at zero. Each forwarded run is
//! a child of the wrapper process that started it, so the ancestors running this same
//! executable are the chain. Linux exposes the tree through `/proc`; elsewhere the
//! environment counter is the only guard.

/// Bound on how far up the process tree one check walks.
#[cfg(target_os = "linux")]
const MAX_ANCESTORS: usize = 4096;

/// Number of ancestors of this process that run the same executable, stopping at `limit`.
#[cfg(target_os = "linux")]
pub(super) fn wrapper_ancestors(limit: u32) -> u32 {
    use std::os::unix::fs::MetadataExt;

    let Some(me) = std::env::current_exe()
        .and_then(std::fs::metadata)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
    else {
        return 0;
    };
    let mut count = 0;
    let mut pid = std::process::id();
    for _ in 0..MAX_ANCESTORS {
        let Some(parent) = parent_of(pid) else { break };
        if parent <= 1 {
            break;
        }
        if executable_of(parent) == Some(me) {
            count += 1;
            if count >= limit {
                break;
            }
        }
        pid = parent;
    }
    count
}

/// The process tree is not readable here, so only the environment counter applies.
#[cfg(not(target_os = "linux"))]
pub(super) fn wrapper_ancestors(_limit: u32) -> u32 {
    0
}

#[cfg(target_os = "linux")]
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // After the parenthesised command name come the state, then the parent pid.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
fn executable_of(pid: u32) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(format!("/proc/{pid}/exe"))
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}
