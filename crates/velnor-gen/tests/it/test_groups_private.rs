// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::SECTION;
use crate::test_groups::{DATABASE, view_with};

#[test]
fn test_groups_with_private_sources_are_refused_naming_both_keys() -> TestResult {
    let config = format!("{DATABASE}\n{SECTION}");
    let error = workflow(&view_with(&["Cargo.toml"], &config, "0.1.90")?)
        .err()
        .ok_or("the groups were accepted and then dropped from the workflow")?;
    let text = error.to_string();
    assert!(
        text.contains("[[test_groups]]") && text.contains("[private_sources]"),
        "{text}"
    );
    Ok(())
}
