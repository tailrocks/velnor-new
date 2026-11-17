// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};

const CONFIG: &str = "[workspace]\nrelease_always = false\ngit_release_draft = true\nsemver_check = false\npublish_no_verify = true\nrelease = false\npr_branch_prefix = \"new-\"\n[[package]]\nname = \"app\"\nrelease = true\ngit_tag_name = \"app-v{{ version }}\"\n";

fn run(label: &str, args: &[&str]) -> Output {
    let dir = std::env::temp_dir().join(format!("velnor-ci-hyphen-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("release-plz.toml"), CONFIG).unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .env_remove("GITHUB_TOKEN")
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ignored = std::fs::remove_dir_all(&dir);
    output
}

fn showed_help(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stdout).contains("Usage:")
}

fn refused_after_comparing(output: &Output) -> bool {
    output.status.code() == Some(1) && !showed_help(output)
}

#[test]
fn a_hyphen_prefix_is_compared_and_a_mismatch_refused() {
    for prefix in ["-h", "--help", "-x", "--version"] {
        let output = run(
            "prefix",
            &[
                "release",
                "check-config",
                "release-plz.toml",
                "--branch-prefix",
                "release-plz.toml",
                prefix,
            ],
        );
        assert!(refused_after_comparing(&output), "{prefix}: {output:?}");
    }
}

#[test]
fn a_hyphen_prefix_before_another_pair_does_not_swallow_the_next_option() {
    let output = run(
        "prefix-pairs",
        &[
            "release",
            "check-config",
            "release-plz.toml",
            "--branch-prefix",
            "release-plz.toml",
            "-h",
            "--manifests",
        ],
    );
    assert!(refused_after_comparing(&output), "{output:?}");
}

#[test]
fn a_hyphen_tag_pattern_is_compared_and_a_mismatch_refused() {
    for pattern in ["-h", "--help", "-app-v{{ version }}"] {
        let output = run(
            "tag",
            &[
                "release",
                "check-config",
                "release-plz.toml",
                "--tag-pattern",
                "release-plz.toml",
                pattern,
                "--repo=o/n",
            ],
        );
        assert!(refused_after_comparing(&output), "{pattern}: {output:?}");
    }
}

#[test]
fn a_hyphen_file_name_in_a_pair_is_a_value_not_a_flag() {
    for pair in [
        ["--branch-prefix", "-h", "x-", "--manifests"],
        ["--tag-pattern", "-h", "x-", "--repo=o/n"],
    ] {
        let output = run(
            "file",
            &[
                "release",
                "check-config",
                "release-plz.toml",
                pair[0],
                pair[1],
                pair[2],
                pair[3],
            ],
        );
        assert!(refused_after_comparing(&output), "{pair:?}: {output:?}");
    }
}

const VERIFIED: &str = "--verified-commit=1111111111111111111111111111111111111111";

fn assert_no_flag(label: &str, cases: &[Vec<&str>]) {
    for args in cases {
        let output = run(label, args);
        assert!(!showed_help(&output), "{args:?}: {output:?}");
        assert_ne!(output.status.code(), Some(0), "{args:?}: {output:?}");
    }
}

#[test]
fn no_gate_record_or_preflight_option_reads_a_hyphen_value_as_a_flag() {
    assert_no_flag(
        "read",
        &[
            vec!["release-gate", "--repo", "-h", "--sha=a"],
            vec!["release-gate", "--repo=o/n", "--sha", "-h"],
            vec![
                "release-gate",
                "--repo=o/n",
                "--sha=a",
                "--pr-only-check",
                "-h",
            ],
            vec!["release", "record", "--repo", "-h", VERIFIED, "out.json"],
            vec![
                "release",
                "record",
                "--repo=o/n",
                "--verified-commit",
                "-h",
                "out.json",
            ],
            vec![
                "release",
                "preflight",
                "--repo",
                "-h",
                VERIFIED,
                "--branch-prefix=x",
            ],
            vec![
                "release",
                "preflight",
                "--repo=o/n",
                "--verified-commit",
                "-h",
                "--branch-prefix=x",
            ],
            vec![
                "release",
                "check-config",
                "release-plz.toml",
                "--repo",
                "-h",
            ],
        ],
    );
}

#[test]
fn no_publish_option_reads_a_hyphen_value_as_a_flag() {
    assert_no_flag(
        "publish",
        &[
            vec![
                "release",
                "publish",
                "--repo",
                "-h",
                "--tag=v1",
                "--app=a",
                "--no-files",
            ],
            vec![
                "release",
                "publish",
                "--repo=o/n",
                "--tag",
                "-h",
                "--app=a",
                "--no-files",
            ],
            vec![
                "release",
                "publish",
                "--repo=o/n",
                "--tag=v1",
                "--app",
                "-h",
                "--no-files",
            ],
            vec![
                "release",
                "publish",
                "--repo=o/n",
                "--tag=v1",
                "--app=a",
                "--dir",
                "-h",
                "--no-files",
            ],
        ],
    );
}
