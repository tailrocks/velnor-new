// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Invocation;

fn words(line: &str) -> Vec<String> {
    line.split(' ').map(str::to_owned).collect()
}

#[test]
fn the_no_run_preflight_drops_the_fail_fast_flags_nextest_refuses_beside_no_run() {
    for flag in ["--no-fail-fast", "--fail-fast"] {
        let command = words(&format!(
            "cargo-nextest nextest run --workspace --locked {flag} --profile ci"
        ));
        let preflight = Invocation::new(&command)
            .preflight
            .unwrap_or_else(|| unreachable!("a nextest run has a preflight"));
        assert!(preflight.contains(&"--no-run".to_owned()), "{preflight:?}");
        assert!(!preflight.contains(&flag.to_owned()), "{preflight:?}");
        assert!(preflight.contains(&"--profile".to_owned()), "{preflight:?}");
    }
}

#[test]
fn the_preflight_keeps_the_arguments_after_the_separator_untouched() {
    let command = words("cargo test --workspace -- --no-fail-fast");
    let preflight = Invocation::new(&command)
        .preflight
        .unwrap_or_else(|| unreachable!("a cargo test has a preflight"));
    assert_eq!(
        preflight,
        words("cargo test --workspace --no-run -- --no-fail-fast")
    );
}
