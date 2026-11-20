// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::delta::{compiled, run, success};
use crate::io::Repo;
use std::fs::File;

#[test]
fn an_oversized_delta_notice_names_its_measured_size_and_the_limit() {
    let repo = Repo::new("delta-cap-notice");
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let huge =
        File::create(repo.root().join("target/huge")).unwrap_or_else(|e| unreachable!("{e}"));
    huge.set_len(499_999_995)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, save) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(save, "save=false\n");
    let notice = String::from_utf8_lossy(&output.stdout).into_owned();
    let numbers: Vec<u64> = notice
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|digits| digits.parse().ok())
        .collect();
    assert!(numbers.contains(&500_000_000), "{notice}");
    assert!(
        numbers
            .iter()
            .any(|n| (500_000_001..500_004_096).contains(n)),
        "the measured size must be named: {notice}"
    );
    assert!(!repo.root().join("delta").exists());
}
