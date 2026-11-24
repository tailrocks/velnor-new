// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_ci::cli::ImageSysrootArgs;
use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_deploy::lock::parse_lock;
use velnor_ci::io::image_deploy::sysroot::unpack_with;

use super::transport::{Fake, Reply};
use crate::support::fresh_dir;

const DPKG: &str = r#"#!/bin/sh
[ "$1" = "--extract" ] || exit 2
root="$3"
while read -r kind path rest; do
  case "$kind" in
    file) /bin/mkdir -p "$root/${path%/*}"; printf '%s' "$rest" > "$root/$path" ;;
    link) /bin/mkdir -p "$root/${path%/*}"; /bin/ln -s "$rest" "$root/$path" ;;
    fail) exit 1 ;;
  esac
done < "$2"
"#;
const LIB: &str = "usr/lib/x86_64-linux-gnu";
const RUNTIME_URL: &str = "https://snapshot.test/libssl3t64_3.5.0_amd64.deb";
const DEV_URL: &str = "https://snapshot.test/libssl-dev_3.5.0_amd64.deb";

const RUNTIME_DEB: &str = "file usr/lib/x86_64-linux-gnu/libssl.so.3 ssl\nfile usr/lib/x86_64-linux-gnu/libcrypto.so.3 crypto\n";
const DEV_DEB: &str = "link usr/lib/x86_64-linux-gnu/libssl.so libssl.so.3\nlink usr/lib/x86_64-linux-gnu/libcrypto.so libcrypto.so.3\nfile usr/lib/x86_64-linux-gnu/libssl.a static\nfile usr/include/openssl/ssl.h ssl-header\nfile usr/include/x86_64-linux-gnu/openssl/configuration.h configuration\n";

struct Setup {
    dir: velnor_test_support::ScratchDir,
}

fn lock(entries: &[(&str, &str, &str)]) -> String {
    let mut text = "version = 1\n".to_owned();
    for (package, url, sha) in entries {
        let _ = write!(
            text,
            "\n[[deb]]\npackage = \"{package}\"\nurl = \"{url}\"\nsha256 = \"{sha}\"\n"
        );
    }
    text
}

impl Setup {
    fn new(name: &str, lock_text: &str) -> Self {
        let dir = fresh_dir("deploy", name);
        fs::write(dir.join("image-sysroot.lock"), lock_text)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let dpkg = dir.join("dpkg-deb");
        fs::write(&dpkg, DPKG).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&dpkg, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        Self { dir }
    }

    fn args(&self) -> ImageSysrootArgs {
        ImageSysrootArgs {
            lock: self.dir.join("image-sysroot.lock"),
            out: self.dir.join("sysroot"),
            dpkg_deb: self.dir.join("dpkg-deb"),
        }
    }

    fn environment(&self) -> PathBuf {
        self.dir.join("github-env")
    }

    fn unpack(&self, fake: &Fake) -> Result<String, String> {
        unpack_with(&self.args(), fake, Some(&self.environment())).map_err(|e| e.to_string())
    }
}

fn good_lock() -> String {
    lock(&[
        (
            "libssl3t64",
            RUNTIME_URL,
            &sha256_hex(RUNTIME_DEB.as_bytes()),
        ),
        ("libssl-dev", DEV_URL, &sha256_hex(DEV_DEB.as_bytes())),
    ])
}

fn good_fake() -> Fake {
    Fake::default()
        .with("GET", RUNTIME_URL, Reply::bytes(RUNTIME_DEB.as_bytes()))
        .with("GET", DEV_URL, Reply::bytes(DEV_DEB.as_bytes()))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn the_locked_packages_are_unpacked_into_openssl_directories_and_the_environment_names_them() {
    let setup = Setup::new("sysroot-ok", &good_lock());
    let report = setup.unpack(&good_fake());
    assert!(report.is_ok_and(|text| text.contains("unpacked 2 packages")));
    let root = setup.dir.join("sysroot/root");
    assert_eq!(read(&root.join(LIB).join("libssl.so")), "ssl");
    assert!(
        fs::symlink_metadata(root.join(LIB).join("libcrypto.so"))
            .is_ok_and(|m| m.file_type().is_symlink())
    );
    assert!(!root.join(LIB).join("libssl.a").exists());
    assert_eq!(
        read(&root.join("usr/include/openssl/configuration.h")),
        "configuration"
    );
    assert_eq!(read(&root.join("usr/include/openssl/ssl.h")), "ssl-header");
    let environment = read(&setup.environment());
    assert!(environment.contains(&format!(
        "X86_64_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR={}",
        root.join(LIB).display()
    )));
    assert!(environment.contains(&format!(
        "X86_64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR={}",
        root.join("usr/include").display()
    )));
}

#[test]
fn a_package_that_does_not_match_its_locked_sha256_is_refused_before_it_is_unpacked() {
    let setup = Setup::new("sysroot-tampered", &good_lock());
    let fake = Fake::default()
        .with(
            "GET",
            RUNTIME_URL,
            Reply::bytes(b"file usr/lib/x86_64-linux-gnu/libssl.so.3 evil\n"),
        )
        .with("GET", DEV_URL, Reply::bytes(DEV_DEB.as_bytes()));
    let message = setup.unpack(&fake).err().unwrap_or_default();
    assert!(
        message.contains("libssl3t64 does not match the sha256"),
        "{message}"
    );
    assert!(!setup.dir.join("sysroot/root").join(LIB).exists());
    assert!(!setup.environment().exists());
}

#[test]
fn packages_without_the_openssl_libraries_a_failing_unpacker_and_a_bad_download_are_refused() {
    let only_runtime = lock(&[(
        "libssl3t64",
        RUNTIME_URL,
        &sha256_hex(RUNTIME_DEB.as_bytes()),
    )]);
    let setup = Setup::new("sysroot-incomplete", &only_runtime);
    let message = setup.unpack(&good_fake()).err().unwrap_or_default();
    assert!(message.contains("do not provide libssl.so"), "{message}");
    let failing = "fail\n";
    let setup = Setup::new(
        "sysroot-failing",
        &lock(&[("broken", RUNTIME_URL, &sha256_hex(failing.as_bytes()))]),
    );
    let fake = Fake::default().with("GET", RUNTIME_URL, Reply::bytes(failing.as_bytes()));
    assert!(
        setup
            .unpack(&fake)
            .err()
            .unwrap_or_default()
            .contains("could not unpack")
    );
    let setup = Setup::new("sysroot-down", &good_lock());
    let down = Fake::default().with("GET", RUNTIME_URL, Reply::json(503, ""));
    assert!(
        setup
            .unpack(&down)
            .err()
            .unwrap_or_default()
            .contains("HTTP 503")
    );
}

#[test]
fn a_redirect_is_followed_only_to_https() {
    let setup = Setup::new("sysroot-redirect", &good_lock());
    let moved = Fake::default()
        .with(
            "GET",
            RUNTIME_URL,
            Reply::redirect("https://mirror.test/runtime.deb"),
        )
        .with(
            "GET",
            "https://mirror.test/runtime.deb",
            Reply::bytes(RUNTIME_DEB.as_bytes()),
        )
        .with("GET", DEV_URL, Reply::bytes(DEV_DEB.as_bytes()));
    assert!(setup.unpack(&moved).is_ok());
    let relative = Fake::default()
        .with("GET", RUNTIME_URL, Reply::redirect("/file/abc/runtime.deb"))
        .with(
            "GET",
            "https://snapshot.test/file/abc/runtime.deb",
            Reply::bytes(RUNTIME_DEB.as_bytes()),
        )
        .with("GET", DEV_URL, Reply::bytes(DEV_DEB.as_bytes()));
    assert!(setup.unpack(&relative).is_ok());
    let protocol_relative =
        Fake::default().with("GET", RUNTIME_URL, Reply::redirect("//evil.test/a.deb"));
    assert!(setup.unpack(&protocol_relative).is_err());
    let downgraded = Fake::default().with(
        "GET",
        RUNTIME_URL,
        Reply::redirect("http://mirror.test/runtime.deb"),
    );
    let message = setup.unpack(&downgraded).err().unwrap_or_default();
    assert!(
        message.contains("redirected somewhere other than https"),
        "{message}"
    );
}

#[test]
fn a_lock_that_could_fetch_something_unlisted_is_refused() {
    let sha = "a".repeat(64);
    let good = lock(&[("libssl3t64", RUNTIME_URL, &sha)]);
    assert!(parse_lock(&good).is_ok());
    let cases = [
        lock(&[("libssl3t64", "http://snapshot.test/a.deb", &sha)]),
        lock(&[("libssl3t64", "https://snapshot.test/a.tar", &sha)]),
        lock(&[("libssl3t64", "https://snapshot.test/a.deb?x=1", &sha)]),
        lock(&[("libssl3t64", RUNTIME_URL, &"A".repeat(64))]),
        lock(&[("libssl3t64", RUNTIME_URL, "abc")]),
        lock(&[("lib ssl", RUNTIME_URL, &sha)]),
        lock(&[("a", RUNTIME_URL, &sha), ("a", DEV_URL, &sha)]),
        "version = 1\n".to_owned(),
        good.replace("version = 1", "version = 2"),
        good.replace("package =", "color = 1\npackage ="),
    ];
    for text in cases {
        assert!(parse_lock(&text).is_err(), "{text}");
    }
}
