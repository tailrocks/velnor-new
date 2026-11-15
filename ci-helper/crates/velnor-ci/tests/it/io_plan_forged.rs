// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io_plan::{plan_command, plan_repo};

#[test]
fn plan_explain_prints_a_hostile_key_in_one_line_that_is_no_workflow_command() {
    let repo = plan_repo("forged");
    repo.write(
        ".config/nextest.toml",
        "[scripts.wrapper.w]\ncommand = \"true\"\n[profile.\"p\\n::warning::FORGED\"]\nscripts = [{ filter = \"all()\", run-wrapper = \"w\" }]\n",
    );
    repo.write(
        ".cargo/config.toml",
        "[target.\"x\\n::error::FORGED2\"]\nrunner = \"true\"\n",
    );
    let output = plan_command(&repo)
        .args([
            "plan",
            "--explain",
            "--changed",
            ".config/nextest.toml",
            ".cargo/config.toml",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    let commands: Vec<&str> = text.lines().filter(|line| line.starts_with("::")).collect();
    assert_eq!(commands.len(), 2, "{text}");
    assert!(
        commands
            .iter()
            .all(|line| line.starts_with("::warning file=")),
        "{text}"
    );
    assert!(text.contains("%0A::error::FORGED2"), "{text}");
    assert!(text.contains("%0A::warning::FORGED"), "{text}");
}
