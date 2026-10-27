// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

#[path = "public_source_lineage/fixture.rs"]
mod fixture;

#[test]
fn public_source_lineage_child() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_LINEAGE_CHILD").is_none() {
        return Ok(());
    }
    let result = fixture::call_api();
    let output = match result {
        Ok(tip) => serde_json::json!({"ok":true,"tip":tip}),
        Err(error) => serde_json::json!({"ok":false,"error":format!("{error:?}")}),
    };
    fs::write(std::env::var("VELNOR_LINEAGE_RESULT")?, output.to_string())?;
    Ok(())
}

#[test]
fn oversized_commit_payloads_keep_public_lineage_verification_working() -> Result<(), Box<dyn Error>>
{
    for case in [
        "large-source-rest",
        "large-authority-rest",
        "large-tip-rest",
        "valid-git-data",
    ] {
        let observed = fixture::run(case)?;
        assert_eq!(observed, Ok(fixture::EXPECTED_TIP.to_owned()), "{case}");
    }
    Ok(())
}

#[test]
fn public_lineage_rejects_invalid_repository_branch_and_commit_identity()
-> Result<(), Box<dyn Error>> {
    for (case, expected) in [
        ("wrong-repository-id", "package_publish_repository_mismatch"),
        (
            "wrong-repository-name",
            "package_publish_repository_mismatch",
        ),
        (
            "wrong-default-branch",
            "package_publish_default_branch_mismatch",
        ),
        ("unprotected-branch", "package_public_main_not_protected"),
        ("wrong-branch-name", "package_public_main_not_protected"),
        ("malformed-tip", "package_public_main_not_protected"),
        ("moving-final-tip", "package_public_preview_tip_changed"),
        (
            "malformed-source-input",
            "package_public_preview_ancestry_input_invalid",
        ),
        (
            "source-sha-mismatch",
            "package_public_preview_commit_mismatch",
        ),
        (
            "authority-sha-mismatch",
            "package_public_preview_commit_mismatch",
        ),
        ("tip-sha-mismatch", "package_public_preview_commit_mismatch"),
    ] {
        assert_error(case, expected)?;
    }
    Ok(())
}

#[test]
fn public_lineage_rejects_invalid_comparison_and_parent_evidence() -> Result<(), Box<dyn Error>> {
    for (case, expected) in [
        ("wrong-base", "package_publish_preview_ancestry_rejected"),
        (
            "wrong-merge-base",
            "package_publish_preview_ancestry_rejected",
        ),
        ("wrong-status", "package_publish_preview_ancestry_rejected"),
        (
            "wrong-ahead-count",
            "package_publish_preview_ancestry_rejected",
        ),
        ("zero-count", "package_publish_preview_ancestry_rejected"),
        (
            "excessive-count",
            "package_publish_preview_ancestry_rejected",
        ),
        (
            "wrong-second-leg-base",
            "package_publish_preview_ancestry_rejected",
        ),
        (
            "wrong-second-leg-status",
            "package_publish_preview_ancestry_rejected",
        ),
        (
            "missing-compare-fields",
            "package_publish_preview_compare_invalid",
        ),
        (
            "missing-parent",
            "package_publish_preview_compare_head_mismatch",
        ),
        (
            "wrong-parent",
            "package_publish_preview_compare_head_mismatch",
        ),
        (
            "wrong-final-page-head",
            "package_publish_preview_compare_head_mismatch",
        ),
        (
            "large-git-data-b-capped",
            "category=stdout_limit cap=1048576",
        ),
        ("malformed-json", "package_publish_api_json"),
    ] {
        assert_error(case, expected)?;
    }
    Ok(())
}

fn assert_error(case: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    match fixture::run(case)? {
        Ok(tip) => Err(format!("{case}: unexpected success at {tip}").into()),
        Err(error)
            if expected
                .split_ascii_whitespace()
                .all(|part| error.contains(part)) =>
        {
            Ok(())
        }
        Err(error) => Err(format!("{case}: expected {expected}, got {error}").into()),
    }
}
