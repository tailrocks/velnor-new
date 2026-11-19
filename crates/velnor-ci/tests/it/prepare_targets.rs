// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::prepare::{Setup, prepare, reported};
use super::prepare_inputs::git_add;

#[test]
fn prepare_accepts_a_linked_target_with_only_untracked_files() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.populate = Some(Box::new(|checkout| {
        fs::create_dir_all(checkout.join("untracked-output"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        std::os::unix::fs::symlink("untracked-output", checkout.join("target"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }));
    let run = prepare("linked-untracked-target", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
}

#[test]
fn prepare_refuses_a_tracked_target_dir_that_differs_from_the_one_recorded_at_generation() {
    let run_with_recorded = |name: &str, recorded: &'static str| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.repo_cargo_config = Some("[build]\ntarget-dir = \"moved\"\n");
        setup.populate = Some(Box::new(move |checkout| {
            git_add(checkout, &[".cargo/config.toml"]);
            fs::write(
                checkout.join(".velnor/rust-metadata.json"),
                format!(
                    "{{\"roots\":{{\"Cargo.toml\":{{\"target_dir\":\"{recorded}\",\"members\":[]}}}},\"manifests\":{{}},\"layout\":\"\"}}"
                ),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }));
        prepare(name, &setup)
    };
    let moved = run_with_recorded("recorded-target-moved", "out");
    assert!(!moved.output.status.success(), "{:?}", moved.output);
    let stderr = String::from_utf8_lossy(&moved.output.stderr);
    assert!(
        stderr.contains("moved") && stderr.contains("out"),
        "{stderr}"
    );
    assert!(stderr.contains(".cargo/config.toml"), "{stderr}");

    let same = run_with_recorded("recorded-target-same", "moved");
    assert!(same.output.status.success(), "{:?}", same.output);
}
