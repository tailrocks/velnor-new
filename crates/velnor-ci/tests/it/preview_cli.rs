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

#[test]
fn the_hard_wired_names_are_the_ones_of_the_contract() {
    let contract = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../velnor-actions-contract/src/source_preview.rs"),
    )
    .unwrap_or_default();
    for expected in [
        velnor_ci::preview::PUBLICATION_AUTHORITY_SHA,
        velnor_ci::preview::REPOSITORY,
        velnor_ci::preview::SIGNER_WORKFLOW,
        velnor_ci::preview::CI_WORKFLOW,
        velnor_ci::preview::QUALIFICATION_WORKFLOW,
    ] {
        assert!(contract.contains(&format!("\"{expected}\"")), "{expected}");
    }
    assert!(contract.contains("MAX_RECEIPT_BYTES: usize = 65_536"));
    assert!(contract.contains("MAX_PAYLOAD_BYTES: u64 = 268_435_456"));
    assert_eq!(velnor_ci::preview::MAX_RECEIPT, 65_536);
    assert_eq!(velnor_ci::preview::MAX_PAYLOAD, 268_435_456);
}

#[test]
fn the_stage_verifier_pins_the_same_gh_builds_and_authority() {
    let script = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../velnor-actions-mise/src/source_preview/source_preview_stage_verify.py"),
    )
    .unwrap_or_default();
    for hash in [
        "cbfdc94f3a5c0713c0d6df3e0a1d88a59a76b0c52332aad7d21d112f9da2cc0e",
        "b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d",
        "5e4470bb57bf1e7cb5063be940d5dffc5be3e1899d6bf7fe1ff15372f7f4cc29",
        "5f8d7527e2bdf584249b030cdd1550c91d75e8afb40aa3c11573d416817a1260",
        velnor_ci::preview::PUBLICATION_AUTHORITY_SHA,
        velnor_ci::preview::OWNER_ID,
    ] {
        assert!(script.contains(hash), "{hash}");
    }
}
