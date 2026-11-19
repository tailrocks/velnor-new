// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, Instant};

use velnor_ci::io::preview::admit::publish;

use crate::admit_world::AdmitWorld;
use crate::preview_admit::{helper, tree};
use crate::support::fresh_dir;

#[test]
fn publication_stops_at_the_deadline_and_leaves_nothing_behind() {
    let root = fs::canonicalize(fresh_dir("velnor-ci-admit", "deadline"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let temp = root.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let admission = helper(&AdmitWorld::new())
        .0
        .unwrap_or_else(|e| unreachable!("{e}"));
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    assert!(publish(&temp, &root.join("output"), &admission, expired).is_err());
    assert!(tree(&temp).is_empty());
    assert!(!root.join("output").exists());
}
