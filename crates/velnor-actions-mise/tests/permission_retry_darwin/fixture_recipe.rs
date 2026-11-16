// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::process::Command;

use super::support::preprovisioned_library;

const CHILD_MARKER: &str = "VELNOR_FIXTURE_RECIPE_CHILD";
const DOC: &str = "docs/implemented/ci-maintenance/repo-verification.md";
const SECTION: &str = "Local recipe for fixture-dependent tests";

#[test]
fn missing_fixture_names_local_recipe() -> Result<(), Box<dyn Error>> {
    if env::var_os(CHILD_MARKER).is_some() {
        preprovisioned_library()?;
        return Ok(());
    }
    let output = Command::new(env::current_exe()?)
        .args([
            "--exact",
            "permission_retry_darwin::fixture_recipe::missing_fixture_names_local_recipe",
            "--nocapture",
        ])
        .env(CHILD_MARKER, "1")
        .env_remove("VELNOR_GROUP_PROBE_RECEIPT")
        .output()?;
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "run without VELNOR_GROUP_PROBE_RECEIPT passed: {report}"
    );
    assert!(report.contains("VELNOR_GROUP_PROBE_RECEIPT"), "{report}");
    assert!(report.contains(DOC), "{report}");
    assert!(report.contains(SECTION), "{report}");
    Ok(())
}
