// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicU64;

use velnor_test_support::ScratchDir;

use crate::support::fresh_dir_in;

const DEAD_PID: i32 = 2_000_000_000;

#[test]
fn a_dead_leftover_that_cannot_be_deleted_outright_is_swept_before_a_new_dir() {
    let root =
        ScratchDir::new(std::env::temp_dir().join(format!("velnor-sweep-{}", std::process::id())));
    let stale = root.join(format!("leftover-{DEAD_PID}-0-case"));
    let locked = stale.join("locked");
    fs::create_dir_all(&locked).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(locked.join("left"), "stale").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500))
        .unwrap_or_else(|e| unreachable!("{e}"));

    let fresh = fresh_dir_in(&root, &AtomicU64::new(0), "leftover", "case");

    assert!(!stale.exists(), "{}", stale.display());
    assert!(fresh.exists());
}
