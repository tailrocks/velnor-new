// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{TestResult, plz_edited, refusal};

#[test]
fn a_branch_prefix_that_cannot_start_a_git_branch_name_is_refused() -> TestResult {
    for prefix in [
        "a b-", "x..y-", "a@{-", "a~-", "a:-", "a?-", "/a-", "a\\\\b-",
    ] {
        let to = format!("[workspace]\npr_branch_prefix = \"{prefix}\"\n");
        let error = refusal(&plz_edited("[workspace]\n", &to)?);
        assert!(
            matches!(error, Some(ReleaseError::InvalidPrBranchPrefix { .. })),
            "{prefix:?}: {error:?}"
        );
    }
    let fine = plz_edited(
        "[workspace]\n",
        "[workspace]\npr_branch_prefix = \"release/plz-\"\n",
    )?;
    assert_eq!(refusal(&fine), None);
    Ok(())
}
