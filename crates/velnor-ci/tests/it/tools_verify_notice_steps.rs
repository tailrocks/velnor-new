// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::tools_verify::{discarded, job, recorded, rustc};
use crate::tools_verify_notice::check_stdout;

#[test]
fn the_notice_explains_the_two_steps_and_prints_only_the_resolve_command() {
    let job = job("tools-verify-two-steps");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(stdout.contains("the command below only reads"), "{stdout}");
    assert!(stdout.contains("running it again is harmless"), "{stdout}");
    assert!(
        stdout.contains("Step 1:")
            && stdout.contains("Step 2: run the --id command it prints, once"),
        "{stdout}"
    );
    assert!(
        stdout.contains("needed once per run, not once per job")
            && stdout.contains("resolves an entry only when the FIRST ref holds the key"),
        "{stdout}"
    );
    assert_eq!(stdout.matches("cache delete-entry").count(), 1, "{stdout}");
    assert!(!stdout.contains(" --id '"), "{stdout}");
    assert!(
        stdout.contains("cache delete-entry --repo 'tailrocks/velnor-new' --key 'velnor-tools-abc123' --ref 'refs/heads/main'"),
        "{stdout}"
    );
    assert!(discarded(&job));
}
