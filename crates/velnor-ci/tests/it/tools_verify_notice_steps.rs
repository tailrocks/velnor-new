// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_ci::time::parse_utc;

use crate::tools_verify::{Job, discarded, job, recorded, rustc};
use crate::tools_verify_notice::check_stdout;

const REPO: &str = "tailrocks/velnor-new";

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

#[test]
fn the_notice_describes_the_stamp_and_the_token_precedence() {
    let job = job("tools-verify-stamp");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = check_stdout(
        &job,
        &anchor,
        &[("VELNOR_TOOLS_CACHE_KEY", "velnor-tools-abc123")],
    );
    assert!(
        stdout.contains("--restored-before (the time this job's restore began"),
        "{stdout}"
    );
    assert!(
        stdout.contains("prefers an exported GH_TOKEN or GITHUB_TOKEN"),
        "{stdout}"
    );
}

fn printed_command(stdout: &str) -> String {
    let (_, rest) = stdout
        .split_once("Command: `")
        .unwrap_or_else(|| unreachable!("no command in {stdout}"));
    let (command, _) = rest
        .split_once('`')
        .unwrap_or_else(|| unreachable!("unterminated command in {stdout}"));
    command.to_owned()
}

fn run_printed_command(job: &Job, command: &str) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let bin = job.root.join("fake-bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    for (name, body) in [
        ("gh", "#!/bin/sh\necho the-token\n"),
        (
            "velnor-ci",
            "#!/bin/sh\nprintf '%s\\n' \"$GITHUB_API_URL\" \"$GITHUB_TOKEN\" \"$@\"\n",
        ),
    ] {
        let path = bin.join(name);
        fs::write(&path, body).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", command])
        .env_clear()
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_printed_command_quotes_a_hostile_key_and_base_branch_so_a_shell_reads_them_back_unchanged() {
    let job = job("tools-verify-shell-quoting");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    let key = "velnor-tools-a'$(id)'b";
    let base = "a'$(id)'";
    let stdout = check_stdout(
        &job,
        &anchor,
        &[
            ("VELNOR_TOOLS_CACHE_KEY", key),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_REF", "refs/pull/7/merge"),
            ("GITHUB_BASE_REF", base),
        ],
    );
    let mut read_back = run_printed_command(&job, &printed_command(&stdout));
    let stamp = read_back.pop().unwrap_or_default();
    assert_eq!(
        read_back.pop().as_deref(),
        Some("--restored-before"),
        "{stdout}"
    );
    assert!(
        parse_utc(&stamp).is_some() && stamp.ends_with('Z'),
        "{stamp}"
    );
    assert_eq!(
        read_back,
        [
            "https://api.github.com",
            "the-token",
            "cache",
            "delete-entry",
            "--repo",
            REPO,
            "--key",
            key,
            "--ref",
            "refs/pull/7/merge",
            "--ref",
            &format!("refs/heads/{base}"),
            "--ref",
            "refs/heads/main",
        ],
        "{stdout}"
    );
}
