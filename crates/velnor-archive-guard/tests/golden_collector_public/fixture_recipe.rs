// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::process::Command;

use super::support::{Harness, diagnostics_candidate};

const CHILD_MARKER: &str = "VELNOR_FIXTURE_RECIPE_CHILD";
const DOC: &str = "docs/implemented/ci-maintenance/repo-verification.md";
const SECTION: &str = "Local recipe for fixture-dependent tests";

#[test]
fn missing_fixture_names_local_recipe() -> Result<(), Box<dyn Error>> {
    match env::var(CHILD_MARKER).as_deref() {
        Ok("diagnostics") => return diagnostics_candidate().map(drop),
        Ok("clock") => return Harness::new_with_clock().map(drop),
        Ok(other) => return Err(format!("unknown recipe case {other}").into()),
        Err(_) => {}
    }
    for (case, variable) in [
        ("diagnostics", "VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE"),
        ("clock", "VELNOR_QUALIFICATION_CLOCK_DRIVER"),
    ] {
        let output = Command::new(env::current_exe()?)
            .args([
                "--exact",
                "fixture_recipe::missing_fixture_names_local_recipe",
                "--nocapture",
            ])
            .env(CHILD_MARKER, case)
            .env_remove(variable)
            .output()?;
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.status.success(),
            "run without {variable} passed: {report}"
        );
        assert!(report.contains(variable), "{report}");
        assert!(report.contains(DOC), "{report}");
        assert!(report.contains(SECTION), "{report}");
    }
    Ok(())
}
