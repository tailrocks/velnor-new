// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

fn run(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn verify_runtime_fails_closed_without_a_token_or_with_a_bad_run_identity() {
    let base = [
        ("PUBLIC_SOURCE", "2222222222222222222222222222222222222222"),
        ("PUBLIC_RUN_ID", "5"),
        ("PUBLIC_RUN_ATTEMPT", "1"),
        ("RUNNER_TEMP", "/tmp"),
        ("GITHUB_OUTPUT", "/dev/null"),
    ];
    assert_eq!(run(&["verify-runtime"], &base).status.code(), Some(1));
    for (key, value) in [
        ("PUBLIC_RUN_ID", "05"),
        ("PUBLIC_RUN_ID", "0"),
        ("PUBLIC_RUN_ATTEMPT", "1x"),
        ("PUBLIC_RUN_ATTEMPT", ""),
    ] {
        let mut env: Vec<(&str, &str)> = base.iter().filter(|(k, _)| *k != key).copied().collect();
        env.push((key, value));
        env.push(("GH_TOKEN", "t"));
        let out = run(&["verify-runtime"], &env);
        assert_eq!(out.status.code(), Some(1), "{key}={value}");
    }
}

#[test]
fn run_authority_fails_closed_on_an_unknown_mode_or_a_missing_environment() {
    assert_eq!(
        run(
            &["run-authority", "--mode", "sign"],
            &[("GH_TOKEN", "t"), ("RUNNER_TEMP", "/tmp")]
        )
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        run(&["run-authority", "--mode", "draft"], &[])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(
            &["run-authority", "--mode", "draft"],
            &[("GH_TOKEN", "t"), ("RUNNER_TEMP", "/tmp")]
        )
        .status
        .code(),
        Some(1)
    );
    assert_eq!(run(&["run-authority"], &[]).status.code(), Some(2));
}
