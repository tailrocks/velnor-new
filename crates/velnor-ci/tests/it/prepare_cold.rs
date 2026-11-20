// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare::{Setup, output, prepare, reported, scratch_root};

#[test]
fn a_cold_run_says_so_in_an_output_so_the_workflow_can_skip_the_save() {
    let missing = scratch_root("os-release-cold-output").join("no-such-os-release");
    let missing: &'static str = Box::leak(missing.display().to_string().into_boxed_str());
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.env.push(("ImageOS", "ubuntu24"));
    setup.env.push(("VELNOR_OS_RELEASE", missing));
    let cold = prepare("compat-cold-output", &setup);
    assert!(cold.output.status.success(), "{:?}", cold.output);
    assert_eq!(output(&cold, "cold"), "true");

    let mut readable = Setup::new(reported("1.99.1", Some("0.9.151")));
    readable.env.push(("ImageOS", "ubuntu24"));
    let warm = prepare("compat-warm-output", &readable);
    assert!(warm.output.status.success(), "{:?}", warm.output);
    assert!(
        !warm.outputs.contains("cold="),
        "a run that read its identity writes no cold output: {}",
        warm.outputs
    );
}

#[test]
fn a_cold_warning_keeps_an_os_release_path_with_a_line_break_on_one_line() {
    let odd = scratch_root("os-release-cold-warning").join("none\n::stop-commands::token");
    let odd: &'static str = Box::leak(odd.display().to_string().into_boxed_str());
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.env.push(("ImageOS", "ubuntu24"));
    setup.env.push(("VELNOR_OS_RELEASE", odd));
    let cold = prepare("compat-cold-warning", &setup);
    let stdout = String::from_utf8_lossy(&cold.output.stdout);
    assert!(cold.output.status.success(), "{:?}", cold.output);
    assert!(
        stdout.contains("::warning::cannot read the OS identity"),
        "{stdout}"
    );
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
}
