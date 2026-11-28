// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::no_run;

fn words(line: &str) -> Vec<&str> {
    line.split(' ').collect()
}

#[test]
fn the_no_run_form_drops_every_flag_nextest_refuses_beside_no_run() {
    for flag in [
        "--no-fail-fast",
        "--fail-fast",
        "--no-tests=pass",
        "--no-tests=fail",
    ] {
        let built = no_run(&words(&format!("nextest run --locked {flag} --profile ci")));
        assert_eq!(built, words("nextest run --locked --profile ci --no-run"));
    }
}

#[test]
fn the_no_run_form_leaves_what_follows_the_separator_untouched() {
    let built = no_run(&words("cargo test --no-fail-fast -- --no-fail-fast"));
    assert_eq!(built, words("cargo test --no-run -- --no-fail-fast"));
}
