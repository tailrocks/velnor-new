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

const LOCK: &str =
    include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

/// A checkout with the repository root and a nested root `lab`, recorded the way a generator
/// pinned at `pin` records them, with an optional config of the nested root.
fn nested_root(
    name: &str,
    pin: &'static str,
    recorded: &'static str,
    config: Option<&'static str>,
) {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.populate = Some(Box::new(move |checkout| {
        let write = |path: &str, text: &str| {
            let file = checkout.join(path);
            fs::create_dir_all(file.parent().unwrap_or(checkout))
                .unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
        };
        write(".velnor/rust-ci.lock", &LOCK.replace("0.1.81", pin));
        write(
            ".velnor/rust-metadata.json",
            &format!(
                "{{\"roots\":{{\"Cargo.toml\":{{\"target_dir\":\"target\",\"members\":[]}},\"lab/Cargo.toml\":{{\"target_dir\":\"{recorded}\",\"members\":[]}}}},\"manifests\":{{}},\"layout\":\"\"}}"
            ),
        );
        let mut added = vec![".velnor/rust-ci.lock"];
        if let Some(config) = config {
            write("lab/.cargo/config.toml", config);
            added.push("lab/.cargo/config.toml");
        }
        git_add(checkout, &added);
    }));
    let run = prepare(name, &setup);
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert!(run.output.status.success(), "{name}: {stderr}");
}

#[test]
fn prepare_accepts_the_default_folder_of_a_nested_root_that_the_pinned_generator_recorded() {
    nested_root(
        "nested-default-new-pin",
        "0.1.107",
        "target/velnor-roots/lab",
        None,
    );
    nested_root("nested-default-old-pin", "0.1.106", "lab/target", None);
}

#[test]
fn prepare_accepts_the_target_folder_a_nested_root_configures_itself() {
    nested_root(
        "nested-configured",
        "0.1.107",
        "lab/out",
        Some("[build]\ntarget-dir = \"out\"\n"),
    );
}
