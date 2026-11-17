// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, ffi::OsStr, path::Path, time::Duration};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

#[test]
fn homebrew_authenticated_acquisition_is_bounded_and_rejects_changed_or_foreign_evidence()
-> Result<(), Box<dyn Error>> {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/homebrew_qualification_acquire.test.ts");
    let execution = PinnedToolExec::new(
        vec![PinnedTool::Bun],
        OsStr::new("bun"),
        [
            "--no-env-file",
            "--no-install",
            "--config=/dev/null",
            "test",
        ]
        .map(Into::into)
        .into_iter()
        .chain([script.into_os_string()])
        .collect(),
    )?;
    let output = execution
        .command(&ToolCatalog::pinned())?
        .run_bounded(1024 * 1024, Duration::from_secs(60))?;
    assert!(
        output.success,
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("4 pass"),
        "Bun must run the complete acquisition behavior harness"
    );
    Ok(())
}
