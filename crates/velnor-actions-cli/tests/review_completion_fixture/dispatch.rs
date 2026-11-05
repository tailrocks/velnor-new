// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, process::Command};

pub(crate) fn with_test(
    name: &str,
    run: impl FnOnce() -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_some() {
        crate::worker::validate_worker("review", Some(name))?;
        run()?;
        crate::worker::record_success("review", name)?;
    } else {
        let status = Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/api2026_pull_context_fixture/driver.py"
            ))
            .args([
                "review",
                name,
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
            ])
            .status()?;
        if !status.success() {
            return Err(format!("isolated review fixture failed: {status}").into());
        }
    }
    Ok(())
}
