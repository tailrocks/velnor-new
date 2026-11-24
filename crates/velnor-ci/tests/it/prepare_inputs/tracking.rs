// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use crate::prepare::{Setup, prepare, reported, scratch_root};

fn runtime_config(name: &str, body: &'static str, track: bool) -> crate::prepare::Run {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some(body);
    setup.populate = Some(Box::new(move |checkout| {
        if track {
            git_add(checkout, &[".cargo/config.toml"]);
        }
    }));
    prepare(name, &setup)
}

pub(crate) fn git_add(checkout: &Path, paths: &[&str]) {
    let added = Command::new("git")
        .arg("add")
        .args(paths)
        .current_dir(checkout)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(added.success());
}

fn install_git(checkout: &Path, body: &str) {
    let path = checkout
        .parent()
        .unwrap_or_else(|| unreachable!())
        .join("bin/git");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn prepare_refuses_an_untracked_config_that_moves_the_cached_output_paths() {
    for body in [
        "[build]\nbuild-dir = \"out\"\n",
        "[build]\ntarget-dir = \"out\"\n",
        "[build]\ntarget-dir = \"{workspace-root}/out\"\n",
        "[build]\ntarget-dir = \"~/out\"\n",
    ] {
        let run = runtime_config(&format!("refuse-{}", body.len()), body, false);
        assert!(!run.output.status.success(), "{body}");
        let stderr = String::from_utf8_lossy(&run.output.stderr);
        assert!(stderr.contains(".cargo/config.toml"), "{body}: {stderr}");
    }
}

#[test]
fn prepare_accepts_a_tracked_target_dir_and_an_untracked_unrelated_setting() {
    let tracked = runtime_config("accept-tracked", "[build]\ntarget-dir = \"out\"\n", true);
    assert!(tracked.output.status.success(), "{:?}", tracked.output);
    let unrelated = runtime_config("accept-jobs", "[build]\njobs = 2\n", false);
    assert!(unrelated.output.status.success(), "{:?}", unrelated.output);
}

#[test]
fn prepare_refuses_a_target_dir_when_tracking_cannot_be_read() {
    let refuses = |name: &str, plain: bool| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.repo_cargo_config = Some("[build]\ntarget-dir = \"out\"\n");
        setup.plain = plain;
        if !plain {
            setup.populate = Some(Box::new(|checkout| install_git(checkout, "exit 1")));
        }
        let run = prepare(name, &setup);
        assert!(!run.output.status.success(), "{name}: {:?}", run.output);
        let stderr = String::from_utf8_lossy(&run.output.stderr);
        assert!(stderr.contains(".cargo/config.toml"), "{name}: {stderr}");
        assert!(stderr.contains("caches target"), "{name}: {stderr}");
    };
    refuses("no-git-target", true);
    refuses("git-down-target", false);

    let mut broken = Setup::new(reported("1.99.1", Some("0.9.151")));
    broken.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    broken.populate = Some(Box::new(|checkout| install_git(checkout, "exit 1")));
    let run = prepare("git-down-profile", &broken);
    assert!(!run.output.status.success(), "{:?}", run.output);
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("git"));
    assert_eq!(
        (run.env_file, run.outputs, run.velnor_config),
        (String::new(), String::new(), String::new())
    );
}

#[test]
fn prepare_refuses_an_untracked_config_that_shadows_a_tracked_target_dir() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some("[build]\ntarget-dir = \"out\"\n");
    setup.populate = Some(Box::new(|checkout| {
        git_add(checkout, &[".cargo/config.toml"]);
        fs::write(checkout.join(".cargo/config"), "[build]\njobs = 2\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
    }));
    let run = prepare("shadow-target-dir", &setup);
    assert!(!run.output.status.success(), "{:?}", run.output);
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert!(stderr.contains(".cargo/config:"), "{stderr}");
    assert!(stderr.contains("caches out"), "{stderr}");
}

#[test]
fn prepare_accepts_an_untracked_config_that_shadows_the_same_target_dir() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some("[build]\ntarget-dir = \"out\"\n");
    setup.populate = Some(Box::new(|checkout| {
        git_add(checkout, &[".cargo/config.toml"]);
        fs::write(
            checkout.join(".cargo/config"),
            "[build]\ntarget-dir = \"out\"\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    }));
    let run = prepare("shadow-same-target-dir", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
}

#[test]
fn prepare_asks_git_which_of_the_read_configs_are_tracked() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some("[build]\ntarget-dir = \"out\"\n");
    setup.populate = Some(Box::new(|checkout| {
        fs::create_dir_all(checkout.join("nested/.cargo")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(
            checkout.join("nested/.cargo/config.toml"),
            "[build]\njobs = 2\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(
            checkout.join(".velnor/rust-metadata.json"),
            "{\"roots\":{\"Cargo.toml\":{\"members\":[]},\"nested/Cargo.toml\":{\"members\":[]}},\"manifests\":{},\"layout\":\"\"}",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(checkout.join("notes.txt"), "notes\n").unwrap_or_else(|e| unreachable!("{e}"));
        git_add(checkout, &[".cargo/config.toml", "notes.txt"]);
        install_git(
            checkout,
            "printf '%s\\n' \"$*\" >> ../git-args\nPATH=/usr/bin:/bin\nexec git \"$@\"",
        );
    }));
    let run = prepare("ask-git", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    let args = fs::read_to_string(scratch_root("ask-git").join("git-args"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        args.lines().any(|line| line
            == "--literal-pathspecs ls-files -z -- .cargo/config.toml nested/.cargo/config.toml"),
        "{args}"
    );
    assert!(
        args.lines()
            .any(|line| line.ends_with(" ls-files -z --recurse-submodules")),
        "{args}"
    );
}

#[test]
fn prepare_rejects_tracked_target_inputs_before_writing_anything() {
    let mut accepted = Vec::new();
    for (name, target, alias) in [
        ("target-input", "target", false),
        ("configured-input", "out", false),
        ("nested-target-alias", "source", true),
    ] {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        if target == "out" {
            setup.repo_cargo_config = Some("[build]\ntarget-dir = \"out\"\n");
        }
        setup.populate = Some(Box::new(move |checkout| {
            let directory = checkout.join(target).join(".velnor");
            fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(directory.join("inputs.json"), "tracked input\n")
                .unwrap_or_else(|e| unreachable!("{e}"));
            git_add(checkout, &[&format!("{target}/.velnor/inputs.json")]);
            if target == "out" {
                git_add(checkout, &[".cargo/config.toml"]);
            }
            if alias {
                fs::create_dir_all(checkout.join("nested")).unwrap_or_else(|e| unreachable!("{e}"));
                std::os::unix::fs::symlink("../source", checkout.join("nested/target"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
                fs::write(
                    checkout.join(".velnor/rust-metadata.json"),
                    "{\"roots\":{\"Cargo.toml\":{\"members\":[]},\"nested/Cargo.toml\":{\"members\":[]}},\"manifests\":{},\"layout\":\"\"}",
                )
                .unwrap_or_else(|e| unreachable!("{e}"));
            }
            let metadata =
                fs::metadata(directory.join("inputs.json")).unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(
                checkout.join("sentinel-metadata"),
                format!(
                    "{:?} {}",
                    metadata.modified(),
                    metadata.permissions().mode()
                ),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }));
        let run = prepare(name, &setup);
        if run.output.status.success() {
            accepted.push(name);
            continue;
        }
        let stderr = String::from_utf8_lossy(&run.output.stderr);
        assert!(
            stderr.contains("target") && stderr.contains("inputs.json"),
            "{stderr}"
        );
        assert_eq!(
            (run.env_file, run.outputs, run.velnor_config),
            (String::new(), String::new(), String::new())
        );
        let checkout = scratch_root(name).join("checkout");
        let sentinel = checkout.join(target).join(".velnor/inputs.json");
        assert_eq!(
            fs::read_to_string(&sentinel).unwrap_or_default(),
            "tracked input\n"
        );
        let metadata = fs::metadata(&sentinel).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            fs::read_to_string(checkout.join("sentinel-metadata")).unwrap_or_default(),
            format!(
                "{:?} {}",
                metadata.modified(),
                metadata.permissions().mode()
            )
        );
    }
    assert!(
        accepted.is_empty(),
        "accepted tracked target inputs: {accepted:?}"
    );
}
