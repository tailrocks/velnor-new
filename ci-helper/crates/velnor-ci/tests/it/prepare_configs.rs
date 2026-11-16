// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

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

#[test]
fn a_symlinked_config_inside_the_checkout_is_read() {
    let none = compat_with(
        "a_symlinked_config_inside_the_checkout_i",
        "Cargo.toml",
        |_| {},
    );
    let linked = compat_with(
        "a_symlinked_config_inside_the_checkout_i",
        "Cargo.toml",
        |base| {
            write(base, "shared/profile.toml", DEBUG);
            fs::create_dir_all(base.join(".cargo")).unwrap_or_else(|e| unreachable!("{e}"));
            symlink("../shared/profile.toml", base.join(".cargo/config.toml"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        },
    );
    assert_ne!(none, linked);
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
    let stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    assert!(!run.output.status.success(), "{stderr}");
    assert!(stderr.contains(".cargo/config.toml"), "{stderr}");
    assert!(stderr.contains("outside the checkout"), "{stderr}");
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
