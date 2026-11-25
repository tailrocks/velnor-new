// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::build_fixture::Source;

#[test]
fn the_validation_script_gets_no_credentials_and_leaves_no_process_behind() {
    let source = Source::new("script-env");
    source.script("sleep 30 &\nenv > \"$PWD/../script.env\"");
    let run = source.run(
        &[],
        &[
            ("KUBECONFIG", Some("/etc/kube/config")),
            ("CARGO_HOME", Some("/etc/cargo")),
        ],
    );
    assert!(run.ok, "{}", run.stderr);
    let seen = source.text("script.env");
    assert!(seen.contains("PATH="), "{seen}");
    assert!(
        seen.contains("HOME=") && seen.contains("/.velnor-job-image-candidate-"),
        "build-test.sh kept the real home directory: {seen}"
    );
    for hidden in [
        "DOCKER_HOST",
        "KUBECONFIG",
        "CARGO_HOME",
        "DOCKER_CONFIG",
        "FAKE_DIR",
    ] {
        assert!(!seen.contains(hidden), "{hidden} reached build-test.sh");
    }
}

#[test]
fn the_validation_script_home_is_not_inside_the_source_or_the_output_directory() {
    let source = Source::new("script-home");
    source.script("env > \"$PWD/../script.env\"");
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    let home = source
        .text("script.env")
        .lines()
        .find_map(|line| line.strip_prefix("HOME=").map(str::to_owned))
        .unwrap_or_default();
    assert!(home.contains("/.velnor-job-image-candidate-"), "{home}");
    assert!(!home.contains("-script-home/"), "{home}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_process_that_left_the_script_session_is_stopped_before_the_build_runs() {
    let source = Source::new("script-detached");
    source.script(concat!(
        "setsid sh -c 'while :; do echo alive > \"$1\"; sleep 0.02; done' sh \"$PWD/../heartbeat\" >/dev/null 2>&1 &\n",
        "while [ ! -e \"$PWD/../heartbeat\" ]; do sleep 0.01; done"
    ));
    let run = source.run(&[], &[]);
    assert!(run.ok, "{}", run.stderr);
    assert_ne!(
        source.text("heartbeat"),
        "",
        "the detached process never ran"
    );
    std::fs::remove_file(source.path("heartbeat")).unwrap_or_else(|e| unreachable!("{e}"));
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        !source.path("heartbeat").exists(),
        "a process started by build-test.sh was still running after the build"
    );
}
