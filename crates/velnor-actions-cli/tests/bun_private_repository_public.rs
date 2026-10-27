// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

#[test]
fn private_asset_check_rejects_a_different_destination_before_requesting_metadata()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .env_clear()
        .env("RUNNER_TEMP", temp.path())
        .env("VELNOR_INTERNAL_OP", "bun-assets-repository-check-v1")
        .env(
            "VELNOR_BUN_ASSET_REPOSITORY",
            "fixture-owner/private-source",
        )
        .env("GITHUB_REPOSITORY", "fixture-owner/public-consumer")
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("bun_assets_repository_destination_mismatch"),
        "unexpected native check error: {stderr}"
    );
    assert!(!stderr.contains("bun_assets_repository_token_missing"));
    Ok(())
}
