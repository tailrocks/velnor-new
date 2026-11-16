// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::process::Command;

use super::context_fixture::Fixture;

const CHILD_MARKER: &str = "VELNOR_FIXTURE_RECIPE_CHILD";
const DOC: &str = "docs/implemented/ci-maintenance/repo-verification.md";
const SECTION: &str = "Local recipe for fixture-dependent tests";

#[test]
fn missing_fixture_names_local_recipe() -> Result<(), Box<dyn Error>> {
    if env::var_os(CHILD_MARKER).is_some() {
        Fixture::new()?;
        return Ok(());
    }
    let output = Command::new(env::current_exe()?)
        .args([
            "--exact",
            "fixture_recipe::missing_fixture_names_local_recipe",
            "--nocapture",
        ])
        .env(CHILD_MARKER, "1")
        .env_remove("VELNOR_IMAGE_PUBLICATION_FIXTURE")
        .output()?;
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "run without VELNOR_IMAGE_PUBLICATION_FIXTURE passed: {report}"
    );
    assert!(
        report.contains("VELNOR_IMAGE_PUBLICATION_FIXTURE"),
        "{report}"
    );
    assert!(report.contains(DOC), "{report}");
    assert!(report.contains(SECTION), "{report}");
    Ok(())
}
