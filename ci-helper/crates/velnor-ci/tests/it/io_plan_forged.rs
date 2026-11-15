// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io_plan::{plan_command, plan_repo, stale_plan_command};

#[test]
fn plan_explain_prints_a_hostile_key_in_one_line_that_is_no_workflow_command() {
    let repo = plan_repo("forged");
    repo.write(
        ".config/nextest.toml",
        "[scripts.wrapper.\"w\\n::warning::FORGED\"]\ncommand = \"true\"\n[profile.default]\nscripts = [{ filter = \"all()\", run-wrapper = \"w\\n::warning::FORGED\" }]\n",
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

#[test]
fn plan_explain_prints_no_line_that_starts_with_a_command_after_any_leading_whitespace() {
    let repo = plan_repo("forged-space");
    repo.write(".cargo/config.toml", "[target.x]\nrunner = \"true\"\n");
    let leaders = ["  ", "\u{2028}", "\u{a0}", " \u{a0}\u{2029}"];
    let changed = [".cargo/config.toml"];
    for (index, leader) in leaders.iter().enumerate() {
        let dir = format!("{leader}::warning::DIR{index}");
        repo.write(
            &format!("{dir}/Cargo.toml"),
            "[package]\nname = \"w\"\nedition = \"2024\"\n\n[workspace]\n",
        );
        repo.write(&format!("{dir}/src/lib.rs"), "");
        repo.write(&format!("{dir}/Cargo.lock"), "version = 4\n");
    }
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed"])
        .args(changed)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("DIR3"), "{text}");
    let commands: Vec<&str> = text
        .split([
            '\n', '\r', '\u{2028}', '\u{2029}', '\u{85}', '\u{b}', '\u{c}',
        ])
        .filter(|line| line.trim().starts_with("::"))
        .collect();
    assert_eq!(commands.len(), 1, "{text}");
    assert!(commands[0].starts_with("::warning file="), "{text}");
}

#[test]
fn plan_prints_a_root_with_a_control_character_in_its_name_escaped_or_not_at_all() {
    for leader in ["\t", "\u{85}"] {
        let repo = plan_repo("forged-control");
        let dir = format!("{leader}::warning::DIR");
        repo.write(
            &format!("{dir}/Cargo.toml"),
            "[package]\nname = \"w\"\nedition = \"2024\"\n\n[workspace]\n",
        );
        repo.write(&format!("{dir}/src/lib.rs"), "");
        repo.write(&format!("{dir}/Cargo.lock"), "version = 4\n");
        let mut command = if velnor_gen::io::write_metadata(repo.root()).is_ok() {
            plan_command(&repo)
        } else {
            stale_plan_command(&repo)
        };
        let output = command
            .args(["plan", "--explain"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.chars().all(|c| c == '\n' || !c.is_control()),
            "{output:?}"
        );
        assert!(
            text.lines().all(|line| !line.trim().starts_with("::")),
            "{output:?}"
        );
    }
}

#[test]
fn plan_explain_escapes_a_changed_path_that_carries_a_line_separator_and_a_command() {
    for separator in ["\u{85}", "\u{2028}", "\u{2029}"] {
        let repo = plan_repo("forged-reason");
        let changed = format!("a/src/{separator}::error::forged.rs");
        repo.write(&changed, "");
        let output = plan_command(&repo)
            .args(["plan", "--explain", "--changed"])
            .arg(&changed)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("changed:"), "{text}");
        assert!(!text.contains("::error::"), "{text}");
        assert!(text.chars().all(|c| c == '\n' || !c.is_control()), "{text}");
    }
}

#[test]
fn plan_prints_a_setting_with_a_unicode_separator_or_a_legacy_prefix_as_no_command() {
    let repo = plan_repo("forged-setting");
    repo.write(
        ".cargo/config.toml",
        "[target.\"x\\u2028::error::FORGED\\u0085::error::FORGED2 ##[error]FORGED3\"]\nrunner = \"true\"\n",
    );
    for explain in [true, false] {
        let mut command = plan_command(&repo);
        command.arg("plan");
        if explain {
            command.arg("--explain");
        }
        let output = command
            .args(["--changed", ".cargo/config.toml"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("FORGED"), "{text}");
        assert!(!text.contains("##["), "{text}");
        let commands = text
            .split([
                '\n', '\r', '\u{2028}', '\u{2029}', '\u{85}', '\u{b}', '\u{c}',
            ])
            .filter(|line| line.trim().starts_with("::error"))
            .count();
        assert_eq!(commands, 0, "{text}");
    }
}

#[test]
fn plan_explain_escapes_a_changed_path_in_the_all_header_and_the_unknown_path_lines() {
    for separator in ["\u{2028}", "\u{85}"] {
        let repo = plan_repo("forged-header");
        let changed = format!("unknown/{separator}::error::forged.dat");
        let output = plan_command(&repo)
            .args(["plan", "--explain", "--changed"])
            .arg(&changed)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("forged.dat"), "{text}");
        let commands = text
            .split([
                '\n', '\r', '\u{2028}', '\u{2029}', '\u{85}', '\u{b}', '\u{c}',
            ])
            .filter(|line| line.trim().starts_with("::"))
            .count();
        assert_eq!(commands, 0, "{text}");
    }
}
