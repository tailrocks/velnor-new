// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

use super::prepare::{Run, Setup, digest, prepare, reported};

const DEBUG: &str = "[profile.dev.package.\"*\"]\ndebug = true\n";
const OFF: &str = "[profile.dev.package.\"*\"]\ndebug = false\n";

fn write(base: &Path, path: &str, text: &str) {
    let file = base.join(path);
    fs::create_dir_all(file.parent().unwrap_or(base)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
}

fn metadata(base: &Path, root: &str) {
    let text = format!(
        "{{\"roots\":{{\"{root}\":{{\"members\":[]}}}},\"manifests\":{{}},\"layout\":\"\"}}"
    );
    write(base, ".velnor/rust-metadata.json", &text);
}

fn run_with(
    name: &str,
    root: &'static str,
    populate: impl Fn(&Path) + Send + Sync + 'static,
) -> Run {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.populate = Some(Box::new(move |base| {
        metadata(base, root);
        populate(base);
    }));
    prepare(name, &setup)
}

fn compat_with(
    name: &str,
    root: &'static str,
    populate: impl Fn(&Path) + Send + Sync + 'static,
) -> String {
    let run = run_with(name, root, populate);
    assert!(run.output.status.success(), "{:?}", run.output);
    digest(&run).to_owned()
}

#[test]
fn a_config_under_target_or_vendor_of_the_root_is_ignored() {
    let none = compat_with(
        "a_config_under_target_or_vendor_of_the_r",
        "lab/Cargo.toml",
        |_| {},
    );
    let buried = compat_with(
        "a_config_under_target_or_vendor_of_the_r",
        "lab/Cargo.toml",
        |base| {
            write(base, "lab/target/package/x/.cargo/config.toml", DEBUG);
            write(base, "lab/vendor/y/.cargo/config", "not toml [");
            write(base, "node_modules/z/.cargo/config.toml", DEBUG);
        },
    );
    assert_eq!(none, buried);
}

#[test]
fn a_nested_crates_own_config_off_the_roots_ancestor_path_is_ignored() {
    let none = compat_with(
        "a_nested_crates_own_config_off_the_roots",
        "lab/Cargo.toml",
        |_| {},
    );
    let nested = compat_with(
        "a_nested_crates_own_config_off_the_roots",
        "lab/Cargo.toml",
        |base| {
            write(base, "lab/crates/inner/.cargo/config.toml", DEBUG);
            write(base, "other/.cargo/config.toml", DEBUG);
        },
    );
    assert_eq!(none, nested);
}

#[test]
fn an_ancestor_config_between_the_root_and_the_checkout_is_read() {
    let none = compat_with(
        "an_ancestor_config_between_the_root_and_",
        "lab/sub/Cargo.toml",
        |_| {},
    );
    let between = compat_with(
        "an_ancestor_config_between_the_root_and_",
        "lab/sub/Cargo.toml",
        |base| {
            write(base, "lab/.cargo/config.toml", DEBUG);
        },
    );
    let own = compat_with(
        "an_ancestor_config_between_the_root_and_",
        "lab/sub/Cargo.toml",
        |base| {
            write(base, "lab/sub/.cargo/config.toml", DEBUG);
        },
    );
    assert_ne!(none, between);
    assert_ne!(none, own);
}

const PROVIDER: &str =
    "[build]\ntarget-dir = \"out\"\n[registry]\ncredential-provider = [\"./steal\"]\n";

fn git_add(checkout: &Path, paths: &[&str]) {
    let added = Command::new("git")
        .arg("add")
        .args(paths)
        .current_dir(checkout)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(added.success());
}

fn assert_symlink_refused(run: &Run, path: &str) {
    let stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    assert!(!run.output.status.success(), "{stderr}");
    assert!(stderr.contains(path), "{stderr}");
    assert!(stderr.contains("symbolic link"), "{stderr}");
    assert_eq!(run.outputs, "");
}

#[test]
fn prepare_refuses_an_untracked_symlinked_cargo_config() {
    let run = run_with(
        "prepare_refuses_an_untracked_symlinked_",
        "Cargo.toml",
        |base| {
            write(base, "shared/profile.toml", DEBUG);
            fs::create_dir_all(base.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
            symlink("../shared/profile.toml", base.join(".cargo/config.toml"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        },
    );
    assert_symlink_refused(&run, ".cargo/config.toml");
}

#[test]
fn prepare_refuses_a_tracked_symlink_to_a_cargo_config() {
    let run = run_with(
        "prepare_refuses_a_tracked_symlink_to_a_",
        "Cargo.toml",
        |base| {
            write(base, "shared/profile.toml", PROVIDER);
            fs::create_dir_all(base.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
            symlink("../shared/profile.toml", base.join(".cargo/config.toml"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            git_add(base, &["shared/profile.toml", ".cargo/config.toml"]);
        },
    );
    assert_symlink_refused(&run, ".cargo/config.toml");
}

#[test]
fn prepare_refuses_a_tracked_cargo_directory_symlink() {
    let run = run_with(
        "prepare_refuses_a_tracked_cargo_directo",
        "Cargo.toml",
        |base| {
            write(base, "cfg/config.toml", PROVIDER);
            symlink("cfg", base.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
            git_add(base, &["cfg", ".cargo"]);
        },
    );
    assert_symlink_refused(&run, ".cargo");
}

#[test]
fn a_symlinked_config_resolving_outside_the_checkout_fails_closed() {
    let run = run_with(
        "a_symlinked_config_resolving_outside_the",
        "Cargo.toml",
        |base| {
            let outside = base.join("../outside.toml");
            fs::write(&outside, DEBUG).unwrap_or_else(|e| unreachable!("{e}"));
            fs::create_dir_all(base.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
            symlink(&outside, base.join(".cargo/config.toml"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        },
    );
    assert_symlink_refused(&run, ".cargo/config.toml");
}

#[test]
fn prepare_refuses_an_untracked_config_that_includes_another_file() {
    let run = run_with(
        "prepare_refuses_an_untracked_include",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config.toml", "include = [\"hidden.toml\"]\n");
            write(
                base,
                ".cargo/hidden.toml",
                "[build]\ntarget-dir = \"out\"\n",
            );
        },
    );
    let stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    assert!(!run.output.status.success(), "{stderr}");
    assert!(stderr.contains(".cargo/config.toml"), "{stderr}");
    assert!(stderr.contains("include is not supported"), "{stderr}");
    assert_eq!(run.outputs, "");
    let kept = run_with(
        "prepare_accepts_an_untracked_config_wit",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config.toml", "[net]\noffline = true\n");
        },
    );
    assert!(kept.output.status.success(), "{:?}", kept.output);
}

#[test]
fn config_shadows_config_toml_in_prepare() {
    let only_off = compat_with(
        "config_shadows_config_toml_in_prepare",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config", OFF);
        },
    );
    let both = compat_with(
        "config_shadows_config_toml_in_prepare",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config", OFF);
            write(base, ".cargo/config.toml", DEBUG);
        },
    );
    let only_toml = compat_with(
        "config_shadows_config_toml_in_prepare",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config.toml", DEBUG);
        },
    );
    assert_eq!(only_off, both);
    assert_ne!(both, only_toml);
}

#[test]
fn prepare_fails_closed_when_the_recorded_metadata_is_missing() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.populate = Some(Box::new(|base| {
        write(base, "lab/.cargo/config.toml", DEBUG);
        fs::remove_file(base.join(".velnor/rust-metadata.json"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }));
    let run = prepare("prepare_fails_closed_when_the_recorded_m", &setup);
    assert!(!run.output.status.success(), "{:?}", run.output);
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert!(
        stderr.contains(".velnor/rust-metadata.json is missing; run velnor-gen generate"),
        "{stderr}"
    );
    assert_eq!(run.outputs, "");
    assert_eq!(run.env_file, "");
}
