// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const CONFIG: &str = "full_run = \"nightly\"\n[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";
const ROOTS: [&str; 3] = ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];

fn pinned(version: &str) -> Result<String, Box<dyn Error>> {
    let mut view = view(&ROOTS, Some(CONFIG.as_bytes()))?;
    crate::pipeline::pin_helper(&mut view, version)?;
    workflow(&view)
}

#[test]
fn a_helper_with_the_per_root_audit_audits_each_unit_in_its_own_lane() -> TestResult {
    let text = pinned("0.1.102")?;
    assert!(!text.contains("rust-inputs-audit"), "{text}");
    let invocations = crate::archive::plain_invocations(&text);
    assert!(
        invocations.contains("run: velnor-ci inputs audit --root .\n"),
        "{invocations}"
    );
    assert_eq!(
        invocations
            .matches("run: velnor-ci inputs audit --root tools/a\n")
            .count(),
        1,
        "{invocations}"
    );
    assert!(!invocations.contains("--root tools/b"), "{invocations}");
    assert!(
        !invocations.contains("run: velnor-ci inputs audit\n"),
        "{invocations}"
    );
    Ok(())
}

#[test]
fn a_helper_of_0_1_101_keeps_the_separate_audit_job() -> TestResult {
    let text = pinned("0.1.101")?;
    assert!(text.contains("rust-inputs-audit:"), "{text}");
    assert!(!text.contains("inputs audit --root"), "{text}");
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_separate_audit_job() -> TestResult {
    let text = pinned("0.1.100")?;
    assert!(text.contains("rust-inputs-audit:"), "{text}");
    assert!(!text.contains("inputs audit --root"), "{text}");
    Ok(())
}
