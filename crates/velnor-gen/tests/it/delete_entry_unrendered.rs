// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

#[test]
fn no_pinned_helper_renders_the_delete_entry_command() -> TestResult {
    for helper in ["0.1.110", "0.1.111", "0.1.112"] {
        for config in ["mbx_shared_store = true\n", "mbx_shared_store = false\n"] {
            let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
            pin_helper(&mut repo, helper)?;
            let rendered = workflow(&repo)?;
            assert!(
                rendered.contains("tools-verify check"),
                "{helper} {config:?}"
            );
            assert!(
                !rendered.contains("delete-entry"),
                "{helper} {config:?} renders the command that only helper 0.1.112 has"
            );
        }
    }
    Ok(())
}
