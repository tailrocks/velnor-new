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
fn prepare_ignores_include_in_a_config_toml_shadowed_by_config() {
    let run = run_with(
        "prepare_ignores_include_in_shadowed",
        "Cargo.toml",
        |base| {
            write(base, ".cargo/config", "[net]\noffline = true\n");
            write(base, ".cargo/config.toml", "include = [\"hidden.toml\"]\n");
        },
    );
    let stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    assert!(run.output.status.success(), "{stderr}");
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

fn home_config(text: Option<&'static str>) -> Setup {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.cargo_config = text;
    setup
}

#[test]
fn a_cargo_home_config_outside_the_checkout_changes_the_compat_digest() {
    let none = digest(&prepare("home_cfg", &home_config(None))).to_owned();
    let one = prepare(
        "home_cfg",
        &home_config(Some("[build]\nrustflags = [\"-Cx\"]\n")),
    );
    let two = prepare(
        "home_cfg",
        &home_config(Some("[build]\nrustflags = [\"-Cy\"]\n")),
    );
    assert!(one.output.status.success(), "{:?}", one.output);
    assert_ne!(none, digest(&one));
    assert_ne!(digest(&one), digest(&two));
}

#[test]
fn a_foreign_config_above_the_checkout_changes_the_compat_digest() {
    let build = |text: &'static str| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.rust_ci_toml = Some("ci_debuginfo = \"full\"\n");
        setup.populate = Some(Box::new(move |base| {
            write(base.parent().unwrap_or(base), ".cargo/config.toml", text);
        }));
        digest(&prepare("above_cfg", &setup)).to_owned()
    };
    assert_ne!(build("[net]\nretry = 2\n"), build("[net]\nretry = 3\n"));
}

#[test]
fn an_unreadable_or_including_external_config_fails_closed() {
    let mut unreadable = home_config(None);
    unreadable.populate = Some(Box::new(|base| {
        let home = base.parent().unwrap_or(base).join("cargo-home/config.toml");
        fs::create_dir_all(home).unwrap_or_else(|e| unreachable!("{e}"));
    }));
    let run = prepare("unreadable_cfg", &unreadable);
    assert_eq!(run.output.status.code(), Some(1), "{:?}", run.output);
    assert_eq!(run.outputs, "");
    let run = prepare(
        "include_cfg",
        &home_config(Some("include = \"other.toml\"\n")),
    );
    assert_eq!(run.output.status.code(), Some(1), "{:?}", run.output);
    assert_eq!(run.outputs, "");
}

const MARKER: &str = "# velnor-ci owns this file and rewrites it on every prepare\n";

#[test]
fn a_marked_config_outside_the_one_prepare_rewrites_still_changes_the_compat_digest() {
    let build = |retries: &'static str| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.populate = Some(Box::new(move |base| {
            let text = format!("{MARKER}[build]\nrustflags = [\"{retries}\"]\n");
            write(
                base.parent().unwrap_or(base),
                "cargo-home/config.toml",
                &text,
            );
        }));
        digest(&prepare("marked_home_cfg", &setup)).to_owned()
    };
    assert_ne!(build("-Cx"), build("-Cy"));
}

#[test]
fn the_config_prepare_rewrites_stays_out_of_the_compat_digest() {
    let build = |text: &'static str| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.populate = Some(Box::new(move |base| {
            let text = format!("{MARKER}{text}");
            write(base.parent().unwrap_or(base), ".cargo/config.toml", &text);
        }));
        digest(&prepare("own_cfg", &setup)).to_owned()
    };
    assert_eq!(build("[net]\nretry = 2\n"), build("[net]\nretry = 3\n"));
}

#[test]
fn an_external_config_shadowed_by_the_legacy_one_is_never_read() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.populate = Some(Box::new(|base| {
        let home = base.parent().unwrap_or(base).join("cargo-home");
        write(&home, "config", "[net]\nretry = 2\n");
        write(&home, "config.toml", "include = \"other.toml\"\n");
    }));
    let run = prepare("shadowed_cfg", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    let mut changed = Setup::new(reported("1.99.1", Some("0.9.151")));
    changed.populate = Some(Box::new(|base| {
        let home = base.parent().unwrap_or(base).join("cargo-home");
        write(&home, "config", "[net]\nretry = 3\n");
        write(&home, "config.toml", "include = \"other.toml\"\n");
    }));
    assert_ne!(digest(&run), digest(&prepare("shadowed_cfg", &changed)));
}

#[test]
fn an_empty_home_is_no_home_so_the_checkout_config_is_not_hashed_twice() {
    let build = |retry: &'static str| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.repo_cargo_config = Some(retry);
        setup.env.push(("CARGO_HOME", ""));
        setup.env.push(("HOME", ""));
        let run = prepare("empty_home", &setup);
        assert!(run.output.status.success(), "{:?}", run.output);
        digest(&run).to_owned()
    };
    assert_eq!(build("[net]\nretry = 2\n"), build("[net]\nretry = 3\n"));
}

#[cfg(target_os = "linux")]
#[test]
fn external_config_paths_that_differ_only_in_non_utf8_bytes_do_not_collide() {
    use std::os::unix::ffi::OsStringExt;
    let build = |name: &[u8]| {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.cargo_config = Some("[net]\nretry = 2\n");
        setup.cargo_home = Some(std::ffi::OsString::from_vec(name.to_vec()));
        let run = prepare("non_utf8_home", &setup);
        assert!(run.output.status.success(), "{:?}", run.output);
        digest(&run).to_owned()
    };
    assert_ne!(build(b"home-\xff"), build(b"home-\xfe"));
}
