// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::bounded::copy_oracle;
use super::support::{Harness, diagnostics_candidate};

#[test]
fn rejects_unexpected_empty_top_level_golden_directories() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle = harness.directory.join("unexpected-empty-case-oracle");
    copy_oracle(&source, &oracle)?;
    let manifest_before = fs::read(oracle.join("MANIFEST.sha256"))?;
    let case_before = fs::read(oracle.join("cases/nested/plan.txt"))?;
    fs::create_dir(oracle.join("cases/unexpected-empty"))?;
    let output = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unexpected top-level golden entry: unexpected-empty")
    );
    assert_eq!(fs::read(oracle.join("MANIFEST.sha256"))?, manifest_before);
    assert_eq!(fs::read(oracle.join("cases/nested/plan.txt"))?, case_before);
    assert!(oracle.join("cases/unexpected-empty").is_dir());
    Ok(())
}
