// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::prepare::{Setup, digest, prepare, reported};

fn write(base: &Path, path: &str, text: &str) {
    let file = base.join(path);
    fs::create_dir_all(file.parent().unwrap_or(base)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
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
