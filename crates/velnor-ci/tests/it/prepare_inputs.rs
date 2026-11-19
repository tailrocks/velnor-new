// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use super::prepare::{MISE, OsRelease, Setup, digest, prepare, reported, scratch_root};

fn compat_with(name: &str, change: impl FnOnce(&mut Setup)) -> String {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    change(&mut setup);
    let run = prepare(name, &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    digest(&run).to_owned()
}

#[test]
fn compat_follows_a_repository_dependency_profile_override() {
    let none = compat_with("compat-inputs", |_| {});
    let debug = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    });
    let off = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = false\n");
    });
    let unrelated = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[build]\njobs = 2\n");
    });
    assert_ne!(none, debug);
    assert_ne!(debug, off);
    assert_eq!(none, unrelated);
}

#[test]
fn compat_follows_a_gitignored_repository_profile_override() {
    let none = compat_with("compat-ignored", |_| {});
    let ignored = compat_with("compat-ignored", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
        setup.ignored = true;
    });
    assert_ne!(none, ignored);
}

#[test]
fn prepare_requires_git_to_verify_target_ownership() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    setup.plain = true;
    let run = prepare("compat-plain", &setup);
    assert!(!run.output.status.success(), "{:?}", run.output);
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("git"));
    assert_eq!(
        (run.env_file, run.outputs, run.velnor_config),
        (String::new(), String::new(), String::new())
    );
}

#[test]
fn compat_follows_the_runner_image() {
    let image = |value: Option<&'static str>| {
        compat_with("compat-inputs", |setup| {
            if let Some(value) = value {
                setup.env.push(("ImageOS", value));
            }
        })
    };
    let (unset, new, old) = (
        image(None),
        image(Some("ubuntu24")),
        image(Some("ubuntu22")),
    );
    assert_ne!(new, old);
    assert_ne!(new, unset);
    assert_eq!(new, image(Some("ubuntu24")));
}

#[test]
fn compat_follows_the_distro_in_os_release() {
    let release = |name: &str, body: &str| {
        let dir = scratch_root("os-release");
        fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.join(name);
        fs::write(&path, body).unwrap_or_else(|e| unreachable!("{e}"));
        let path: &'static str = Box::leak(path.display().to_string().into_boxed_str());
        compat_with("compat-os-release", |setup| {
            setup.env.push(("VELNOR_OS_RELEASE", path));
        })
    };
    let base = release("a", "ID=ubuntu\nVERSION_ID=\"24.04\"\n");
    assert_eq!(base, release("b", "ID=ubuntu\nVERSION_ID=\"24.04\"\n"));
    assert_ne!(base, release("c", "ID=ubuntu\nVERSION_ID=\"22.04\"\n"));
    assert_ne!(base, release("d", "ID=debian\nVERSION_ID=\"24.04\"\n"));
}

#[test]
fn an_unreadable_os_release_fails_prepare_instead_of_sharing_a_digest() {
    let missing = scratch_root("os-release-missing").join("no-such-os-release");
    let missing: &'static str = Box::leak(missing.display().to_string().into_boxed_str());
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.env.push(("ImageOS", "ubuntu24"));
    setup.env.push(("VELNOR_OS_RELEASE", missing));
    let run = prepare("compat-no-os-release", &setup);
    assert!(!run.output.status.success(), "{:?}", run.output);
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert!(stderr.contains("os-release"), "{stderr}");
    assert!(run.outputs.is_empty(), "{}", run.outputs);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn a_platform_without_os_release_still_prepares_with_its_own_identity() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.os_release = OsRelease::Platform;
    let run = prepare("compat-no-os-release-platform", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_ne!(digest(&run), "");
}

#[test]
fn rust_before_1_71_gets_numeric_debug_values() {
    let mut setup = Setup::new(reported("1.70.0", Some("0.9.151")));
    setup.release = "1.70.0";
    setup.mise_toml = MISE.replace("1.99.1", "1.70.0");
    let run = prepare("old-rust", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert!(
        run.env_file.contains("CARGO_PROFILE_DEV_DEBUG=1\n"),
        "{}",
        run.env_file
    );
    assert!(run.env_file.contains("CARGO_PROFILE_TEST_DEBUG=1\n"));
    assert!(
        run.velnor_config.contains("debug = 0\n"),
        "{}",
        run.velnor_config
    );
    assert!(!run.velnor_config.contains("\"none\""));
}

fn runtime_config(name: &str, body: &'static str, track: bool) -> super::prepare::Run {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some(body);
    setup.populate = Some(Box::new(move |checkout| {
        if track {
            git_add(checkout, &[".cargo/config.toml"]);
        }
    }));
    prepare(name, &setup)
}

fn git_add(checkout: &Path, paths: &[&str]) {
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
